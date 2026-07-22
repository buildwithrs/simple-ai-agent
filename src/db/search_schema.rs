use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

use crate::{errors::AgentError, tool::Tool};

pub struct SearchSchema {
    pub conn: PgPool,
}

impl SearchSchema {
    pub fn new(pool: PgPool) -> Self {
        Self { conn: pool }
    }
}

#[async_trait]
impl Tool for SearchSchema {
    fn name(&self) -> String {
        "search_schema".to_string()
    }

    fn decription(&self) -> String {
        "search table, columns according the query".to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "table name, column name"
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::ToolError("missing query".to_string()))?;

        let q = query.trim();
        if q.is_empty() {
            return Err(AgentError::ToolError("`query` is empty".to_string()));
        }

        let pattern = format!("%{q}%");
        let table_rows = sqlx::query(
            "SELECT table_schema, table_name, table_type
             FROM information_schema.tables
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
               AND (table_name ILIKE $1 OR table_schema ILIKE $1)
             ORDER BY table_schema, table_name
             LIMIT 50",
        )
        .bind(&pattern)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("fetch tables failed: {e}")))?;

        let column_rows = sqlx::query(
            "SELECT table_schema, table_name, column_name, data_type
             FROM information_schema.columns
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
               AND column_name ILIKE $1
             ORDER BY table_schema, table_name, ordinal_position
             LIMIT 500",
        )
        .bind(&pattern)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("fetch tables failed: {e}")))?;

        let tables: Vec<Value> = table_rows
            .into_iter()
            .map(|r| {
                let schema: String = r.get("table_schema");
                let name: String = r.get("table_name");
                let type_: String = r.get("table_type");
                json!({"schema": schema, "name": name, "table_type": type_})
            })
            .collect();

        let columns: Vec<Value> = column_rows
            .into_iter()
            .map(|r| {
                let schema: String = r.get("table_schema");
                let table_name: String = r.get("table_name");
                let col_name: String = r.get("column_name");
                let dt: String = r.get("data_type");
                json!({"table": format!("{schema}.{table_name}"), "column": col_name, "type": dt})
            })
            .collect();

        let res = json!(
            {"tables": tables, "columns": columns}
        );

        serde_json::to_string(&res)
            .map_err(|e| AgentError::ToolError(format!("serialize tables failed: {e}")))
    }
}
