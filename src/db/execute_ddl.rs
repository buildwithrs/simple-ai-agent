use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Instant;

use crate::{
    db::tools::{require_leading_keyword, split_statements},
    errors::AgentError,
    tool::Tool,
};

const ALLOWED: &[&str] = &["CREATE", "ALTER", "DROP", "TRUNCATE"];

pub struct ExecuteDDL {
    pub conn: PgPool,
}

#[async_trait]
impl Tool for ExecuteDDL {
    fn name(&self) -> String {
        "execute_ddl".to_string()
    }

    fn decription(&self) -> String {
        "Run a single CREATE, ALTER, DROP, or TRUNCATE statement against \
         the active PostgreSQL connection. Requires `confirm: true` and \
         refuses to run on read-only connections."
            .to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "The DDL statement (CREATE / ALTER / DROP / TRUNCATE)."
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Must be true. Confirms the user has approved the statement."
                },
            },
            "required": ["sql", "confirm"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let sql = args
            .get("sql")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AgentError::ToolError("execute_ddl: missing required argument `sql`".into())
            })?;

        let confirm = args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !confirm {
            return Err(AgentError::ToolError(
                "execute_ddl: `confirm: true` is required".into(),
            ));
        }

        let keyword = require_leading_keyword(sql, ALLOWED)?;

        if split_statements(sql).len() != 1 {
            return Err(AgentError::ToolError(
                "execute_ddl: refusing multi-statement script; \
                 send exactly one CREATE/ALTER/DROP/TRUNCATE"
                    .into(),
            ));
        }

        let readonly = false;
        if readonly {
            return Err(AgentError::ToolError(
                "execute_ddl: refusing DDL on a read-only connection".into(),
            ));
        }

        let started = Instant::now();
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&self.conn)
            .await
            .map_err(|e| AgentError::ToolError(format!("execute_ddl: {e}")))?;
        let execution_ms = started.elapsed().as_millis() as u64;

        let object = extract_target_identifier(&keyword, sql)
            .unwrap_or_else(|| "<unknown>".to_string());

        let payload = json!({
            "statement":    match_keyword_label(&keyword),
            "object":       object,
            "execution_ms": execution_ms,
        });

        serde_json::to_string(&payload)
            .map_err(|e| AgentError::ToolError(format!("execute_ddl: serialize error: {e}")))
    }
}

fn match_keyword_label(k: &str) -> &'static str {
    match k {
        "CREATE" => "CREATE TABLE",
        "ALTER" => "ALTER TABLE",
        "DROP" => "DROP TABLE",
        "TRUNCATE" => "TRUNCATE TABLE",
        _ => "DDL",
    }
}

fn extract_target_identifier(keyword: &str, sql: &str) -> Option<String> {
    let tokens: Vec<&str> = sql.split_whitespace().collect();
    let upper_tokens: Vec<String> = tokens.iter().map(|t| t.to_ascii_uppercase()).collect();

    let keyword_pos = upper_tokens
        .iter()
        .position(|t| t == keyword)
        .unwrap_or(0);
    let after_kw = tokens.get(keyword_pos + 1)?;

    // Skip the object-class word (`TABLE`, `INDEX`, ...) if present.
    let candidate = if matches!(
        after_kw.to_ascii_uppercase().as_str(),
        "TABLE" | "INDEX" | "VIEW" | "SCHEMA" | "SEQUENCE" | "TYPE"
    ) {
        tokens.get(keyword_pos + 2)?
    } else {
        after_kw
    };

    Some(
        candidate
            .trim_end_matches(|c: char| c == ';' || c == '(' || c == '.')
            .trim_matches('"')
            .to_string(),
    )
}
