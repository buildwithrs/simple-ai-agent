use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::errors::AgentError;

pub mod desc_table;
pub mod list_tables;
pub mod tools;

pub async fn establish_connection(db_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(50)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(10))
        .connect(db_url)
        .await?;

    Ok(pool)
}

pub fn get_schema_table(args: &Value) -> Result<(String, String), AgentError> {
    let table_desc = args
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentError::ToolError("missing table".into()))?;

    match table_desc.split_once(".") {
        Some((s, t)) => Ok((s.to_string(), t.to_string())),
        None => Ok(("public".to_string(), table_desc.to_string()))
    }
}
