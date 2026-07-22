use serde_json::{Value, json};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Row, Column};
use std::time::Duration;

use crate::db::{
    desc_table::DescTable,
    execute_ddl::ExecuteDDL,
    execute_dml::ExecuteDML,
    execute_query::ExecuteQuery,
    list_tables::{ListSchemas, ListTables},
    search_schema::SearchSchema,
};
use crate::errors::AgentError;
use crate::tool::ToolRegistry;

pub mod desc_table;
pub mod execute_ddl;
pub mod execute_dml;
pub mod execute_query;
pub mod list_tables;
pub mod search_schema;

pub async fn establish_connection(db_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(50)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(10))
        .connect(db_url)
        .await?;

    Ok(pool)
}

pub fn register_db_tools(tool_reg: &mut ToolRegistry, pool: PgPool) {
    tool_reg.register(ListTables::new(pool.clone()));
    tool_reg.register(ListSchemas::new(pool.clone()));
    tool_reg.register(DescTable::new(pool.clone()));
    tool_reg.register(ExecuteQuery::new(pool.clone()));
    tool_reg.register(ExecuteDML::new(pool.clone()));
    tool_reg.register(ExecuteDDL::new(pool.clone()));
    tool_reg.register(SearchSchema::new(pool.clone()));
}

pub fn get_schema_table(args: &Value) -> Result<(String, String), AgentError> {
    let table_desc = args
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentError::ToolError("missing table".into()))?;

    match table_desc.split_once(".") {
        Some((s, t)) => Ok((s.to_string(), t.to_string())),
        None => Ok(("public".to_string(), table_desc.to_string())),
    }
}

pub(crate) fn is_safe_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Split a SQL script into individual top-level statements.
pub(crate) fn split_statements(sql: &str) -> Vec<&str> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Reject any SQL that does not start with one of the allowed keywords.
pub(crate) fn require_leading_keyword(sql: &str, allowed: &[&str]) -> Result<String, AgentError> {
    let first = sql
        .split_whitespace()
        .next()
        .ok_or_else(|| AgentError::ToolError("empty SQL".into()))?;

    let first_upper = first.to_ascii_uppercase();
    if allowed.iter().any(|k| k.eq_ignore_ascii_case(&first_upper)) {
        Ok(first_upper)
    } else {
        Err(AgentError::ToolError(format!(
            "refusing statement starting with `{first}`; expected one of: {}",
            allowed.join(", ")
        )))
    }
}

/// Shape a `sqlx::PgPool` result set into the `{rows, columns}` JSON
/// representation shared by all tools.
pub(crate) fn shape_rows(rows: &[PgRow]) -> (Vec<Value>, Vec<Value>) {
    let columns: Vec<Value> = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name(),
                        "type": c.type_info(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let shaped: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (idx, col) in row.columns().iter().enumerate() {
                // Try to get as a JSON value; on failure, fall back to the
                // string form so the model still sees *something* rather
                // than dropping the column silently.
                let v: Value = row
                    .try_get::<serde_json::Value, _>(idx)
                    .unwrap_or_else(|_| {
                        serde_json::Value::String(format!("<unprintable: {}>", col.type_info()))
                    });
                obj.insert(col.name().to_string(), v);
            }
            Value::Object(obj)
        })
        .collect();

    (shaped, columns)
}

/// extract bound parameters in array
pub(crate) fn extract_params(args: &Value) -> Vec<Value> {
    args.get("params")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
