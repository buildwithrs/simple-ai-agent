use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Instant;

use crate::{
    db::{extract_params, require_leading_keyword, split_statements},
    errors::AgentError,
    tool::Tool,
};

const ALLOWED: &[&str] = &["INSERT", "UPDATE", "DELETE", "MERGE"];

pub struct ExecuteDML {
    pub conn: PgPool,
}

impl ExecuteDML {
    pub fn new(pool: PgPool) -> Self {
        Self { conn: pool }
    }
}

#[async_trait]
impl Tool for ExecuteDML {
    fn name(&self) -> String {
        "execute_dml".to_string()
    }

    fn decription(&self) -> String {
        "Run a single INSERT, UPDATE, DELETE, or MERGE statement against \
         the active PostgreSQL connection with optional bound parameters. \
         Always requires user confirmation in the calling layer; the \
         tool itself does not prompt."
            .to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "The DML statement (INSERT / UPDATE / DELETE / MERGE)."
                },
                "params": {
                    "type": "array",
                    "description": "Bound parameters ($1, $2, ...).",
                    "items": {}
                },
                "preview": {
                    "type": "boolean",
                    "description": "If true, run a SELECT COUNT(*) with the same predicate and return the blast radius without executing the DML."
                },
            },
            "required": ["sql"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let sql = args
            .get("sql")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AgentError::ToolError("execute_dml: missing required argument `sql`".into())
            })?;

        let preview = args
            .get("preview")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let params = extract_params(&args);

        // Safety: refuse anything that is not a DML statement.
        let keyword = require_leading_keyword(sql, ALLOWED)?;

        // Refuse multi-statement scripts. The tool is single-purpose.
        if split_statements(sql).len() != 1 {
            return Err(AgentError::ToolError(
                "execute_dml: refusing multi-statement script; \
                 send exactly one INSERT/UPDATE/DELETE/MERGE"
                    .into(),
            ));
        }

        // Preview branch: count before mutating. Only UPDATE/DELETE have
        // a meaningful "blast radius"; INSERT/MERGE short-circuit.
        if preview {
            if keyword == "INSERT" || keyword == "MERGE" {
                let payload = json!({
                    "preview":      true,
                    "would_affect": 0,
                });
                return serde_json::to_string(&payload).map_err(|e| {
                    AgentError::ToolError(format!("execute_dml: serialize error: {e}"))
                });
            }

            let count_sql = build_count_sql(&keyword, sql)?;
            let mut q = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql));
            for p in &params {
                q = q.bind(p);
            }
            let would_affect = q
                .fetch_one(&self.conn)
                .await
                .map_err(|e| AgentError::ToolError(format!("execute_dml preview: {e}")))?;

            let payload = json!({
                "preview":      true,
                "would_affect": would_affect,
            });
            return serde_json::to_string(&payload)
                .map_err(|e| AgentError::ToolError(format!("execute_dml: serialize error: {e}")));
        }

        let started = Instant::now();
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
        for p in &params {
            q = q.bind(p);
        }
        let result = q
            .execute(&self.conn)
            .await
            .map_err(|e| AgentError::ToolError(format!("execute_dml: {e}")))?;
        let execution_ms = started.elapsed().as_millis() as u64;

        let payload = json!({
            "rows_affected": result.rows_affected(),
            "returning":     [],
            "execution_ms":  execution_ms,
        });

        serde_json::to_string(&payload)
            .map_err(|e| AgentError::ToolError(format!("execute_dml: serialize error: {e}")))
    }
}

fn build_count_sql(keyword: &str, sql: &str) -> Result<String, AgentError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let body = trimmed
        .splitn(2, char::is_whitespace)
        .nth(1)
        .unwrap_or("")
        .trim();

    match keyword {
        "DELETE" => Ok(format!(
            "SELECT count(*) FROM {}",
            body.trim().trim_end_matches(';').trim()
        )),
        "UPDATE" => {
            let (table, where_clause) = split_update_body(body);
            if table.is_empty() {
                return Err(AgentError::ToolError(
                    "execute_dml preview: could not extract table name from UPDATE".into(),
                ));
            }
            Ok(match where_clause {
                Some(w) => format!("SELECT count(*) FROM {} {}", table, w.trim()),
                None => format!("SELECT count(*) FROM {}", table),
            })
        }
        _ => unreachable!("keyword filtered upstream"),
    }
}

fn split_update_body(body: &str) -> (String, Option<String>) {
    let tokens: Vec<&str> = body.split_whitespace().collect();
    let upper_tokens: Vec<String> = tokens.iter().map(|t| t.to_ascii_uppercase()).collect();

    // First standalone WHERE token marks the start of the predicate.
    let where_pos = upper_tokens
        .iter()
        .position(|t| t == "WHERE");

    let table_end = match where_pos {
        Some(p) => p,
        None => tokens.len(),
    };

    let table = tokens[..table_end]
        .first()
        .copied()
        .unwrap_or("")
        .trim_end_matches(';')
        .trim_matches('"')
        .to_string();

    let where_clause = where_pos.map(|p| tokens[p..].join(" "));
    let where_clause = where_clause.map(|w| {
        // Drop the leading WHERE keyword from the predicate so we can
        // re-emit it inside `FROM <table> WHERE ...`.
        w.splitn(2, char::is_whitespace)
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string()
    });

    (table, where_clause)
}
