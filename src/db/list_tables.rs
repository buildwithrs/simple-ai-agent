use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

use crate::{errors::AgentError, tool::Tool};

pub struct ListSchemas {
    pub conn: PgPool,
}

pub struct ListTables {
    pub conn: PgPool,
}

impl ListSchemas {
    pub fn new(pool: PgPool) -> Self {
        Self { conn: pool }
    }
}

impl ListTables {
    pub fn new(pool: PgPool) -> Self {
        Self { conn: pool }
    }
}

#[async_trait]
impl Tool for ListSchemas {
    fn name(&self) -> String {
        "list_schemas".to_string()
    }

    fn decription(&self) -> String {
        "List all accessible PostgreSQL schemas".to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, _args: Value) -> Result<String, AgentError> {
        let schemas = sqlx::query_scalar::<_, String>(
            r#"
              SELECT schema_name
              FROM information_schema.schemata
              ORDER BY schema_name
              "#,
        )
        .fetch_all(&self.conn)
        .await
        .map_err(|error| {
            AgentError::ContextError(format!("failed to list PostgreSQL schemas: {error}"))
        })?;

        Ok(json!({
            "schemas": schemas
        })
        .to_string())
    }
}

#[async_trait]
impl Tool for ListTables {
    fn name(&self) -> String {
        "list_tables".to_string()
    }

    fn decription(&self) -> String {
        "List all tables for a schema".to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "schema": {
                    "type": "string",
                    "description": "Schema name to inspect. Defaults to `public`.",
                },
                "include_views": {
                    "type": "boolean",
                    "description": "Include views in the result. Defaults to false."
                }
            },
            "required": [],
            "additionalProperties": false,
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let schema = args
            .get("schema")
            .and_then(Value::as_str)
            .unwrap_or("public");
        let include_views = args
            .get("include_views")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if schema.is_empty() {
            return Err(AgentError::ToolError("bad schema name".to_string()));
        }

        let sql = format!(
            r#"""
        SELECT table_name, table_type FROM {}.information_schema.tables
        WHERE table_schema = $1 AND table_type=ANY($2)
        ORDER BY table_name
        """#,
            schema
        );

        let type_filter: &[&str] = if include_views {
            &["BASE TABLE", "VIEW"]
        } else {
            &["BASE TABLE"]
        };

        let results = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(schema)
            .bind(type_filter)
            .fetch_all(&self.conn)
            .await
            .map_err(|e| AgentError::ToolError(format!("list tables failed: {e}")))?;

        let mut tables = Vec::with_capacity(results.len());
        for row in results {
            let name: String = row
                .try_get("table_name")
                .map_err(|e| AgentError::ToolError(format!("list tables failed: {e}")))?;

            let table_type: String = row
                .try_get("table_type")
                .map_err(|e| AgentError::ToolError(format!("list tables failed: {e}")))?;

            let type_ = match table_type.as_str() {
                "BASE TYPE" => "table",
                "VIEW" => "view",
                other => other,
            };

            tables.push(json!({"name": name, "type": type_}));
        }

        serde_json::to_string(&json!({"tables": tables}))
            .map_err(|e| AgentError::ToolError(e.to_string()))
    }
}
