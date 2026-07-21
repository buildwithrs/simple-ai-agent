use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::{
    db::tools::{extract_params, require_leading_keyword, shape_rows},
    errors::AgentError,
    tool::Tool,
};

const ALLOWED: &[&str] = &["SELECT", "WITH", "EXPLAIN", "SHOW"];

pub struct ExecuteQuery {
    pub conn: PgPool,
}

#[async_trait]
impl Tool for ExecuteQuery {
    fn name(&self) -> String {
        "execute_query".to_string()
    }

    fn decription(&self) -> String {
        "Run a read-only SQL statement (SELECT, WITH, EXPLAIN, SHOW) \
         against the active PostgreSQL connection and return the rows \
         as JSON. Refuses any mutating statement; use execute_dml or \
         execute_ddl for those."
            .to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "The SELECT / WITH / EXPLAIN statement."
                },
                "params": {
                    "type": "array",
                    "description": "Bound parameters ($1, $2, ...).",
                    "items": {}
                },
                "limit": {
                    "type": "integer",
                    "description": "Hard cap on rows returned. Defaults to 500."
                },
            },
            "required": ["sql"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let query = args
            .get("sql")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::ToolError(format!("execute_query: missing query sql")))?;

        let limit = args
            .get("limit")
            .and_then(Value::as_i64)
            .map(|n| n.max(0) as usize)
            .unwrap_or(500);

        let params = extract_params(&args);

        require_leading_keyword(&query, ALLOWED)?;

        let mut q = sqlx::query(sqlx::AssertSqlSafe(query));
        for p in &params {
            q = q.bind(p);
        }
        let rows = q
            .fetch_all(&self.conn)
            .await
            .map_err(|e| AgentError::ToolError(format!("execute_query: {e}")))?;

        let (mut shaped, columns) = shape_rows(&rows);
        let trunc = shaped.len() > limit;
        if trunc {
            shaped.truncate(limit);
        }

        let result = json!({
            "rows": shaped,
            "columns": columns,
            "row_count": shaped.len(),
            "truncated": trunc,
        });

        serde_json::to_string(&result)
            .map_err(|e| AgentError::ToolError(format!("execute_query: serialize error: {e}")))
    }
}
