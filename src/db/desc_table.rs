use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{db::get_schema_table, errors::AgentError, tool::Tool};

pub struct DescTable {
    pub conn: PgPool,
}

impl DescTable {
    pub fn new(pool: PgPool) -> Self {
        Self { conn: pool }
    }

    async fn fetch_columns(&self, schema: &str, table: &str) -> Result<Vec<PgRow>, AgentError> {
        sqlx::query(
            r#"""SELECT column_name, data_type, is_nullable, column_default
        FROM information_schema.columns
        WHERE table_schema = $1 AND table_name=$2
        ORDER BY ordinal_position"""#,
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("columns query: {e}")))
    }

    async fn fetch_primary_key(&self, schema: &str, table: &str) -> Result<Vec<PgRow>, AgentError> {
        sqlx::query(
            "SELECT a.attname
         FROM pg_index i
         JOIN pg_attribute a
           ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
         WHERE i.indrelid = ($1 || '.' || $2)::regclass
           AND i.indisprimary",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("primary key query: {e}")))
    }

    async fn fetch_indexes(&self, schema: &str, table: &str) -> Result<Vec<PgRow>, AgentError> {
        sqlx::query(
            "SELECT indexname, indexdef
         FROM pg_indexes
         WHERE schemaname = $1 AND tablename = $2
         ORDER BY indexname",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("indexes query: {e}")))
    }

    async fn fetch_foreign_keys(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<PgRow>, AgentError> {
        sqlx::query(
            "SELECT
    kcu.column_name,
    ccu.table_schema  AS foreign_table_schema,
    ccu.table_name    AS foreign_table_name,
    ccu.column_name   AS foreign_column_name,
    rc.update_rule,
    rc.delete_rule
    FROM information_schema.table_constraints AS tc
    JOIN information_schema.key_column_usage  AS kcu
        ON tc.constraint_name = kcu.constraint_name
    AND tc.table_schema   = kcu.table_schema
    JOIN information_schema.constraint_column_usage AS ccu
        ON ccu.constraint_name = tc.constraint_name
    JOIN information_schema.referential_constraints AS rc
        ON rc.constraint_name = tc.constraint_name
    WHERE
        tc.constraint_type = 'FOREIGN KEY'
        AND tc.table_schema = $1
        AND tc.table_name   = $2",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.conn)
        .await
        .map_err(|e| AgentError::ToolError(format!("foreign keys query: {e}")))
    }

    pub fn shape_columns(rows: &[PgRow]) -> Vec<Value> {
        rows.iter()
            .map(|r| {
                let name: String = r.get("column_name");
                let ty: String = r.get("data_type");
                let nullable: String = r.get("is_nullable");
                let default: Option<String> = r.try_get("column_default").ok();
                json!({
                    "name":           name,
                    "type":           ty,
                    "nullable":       nullable == "YES",
                    "default":        default,
                    "is_primary_key": false, // filled in by mark_primary_key_columns
                })
            })
            .collect()
    }

    pub fn shape_primary_key(rows: &[PgRow]) -> Vec<String> {
        rows.iter().map(|r| r.get::<String, _>("attname")).collect()
    }

    /// Flip `is_primary_key` on the column entries whose name appears in
    /// `pk_set`. Mutates `columns` in place.
    pub fn mark_primary_key_columns(columns: &mut [Value], pk_set: &[String]) {
        for col in columns.iter_mut() {
            let n = col.get("name").and_then(Value::as_str).unwrap_or("");
            if pk_set.iter().any(|k| k == n) {
                col["is_primary_key"] = json!(true);
            }
        }
    }

    pub fn shape_indexes(idx_rows: &[PgRow]) -> Vec<Value> {
        idx_rows
            .iter()
            .map(|r| {
                let name: String = r.get("indexname");
                let def: String = r.get("indexdef");
                json!({
                    "name":    name,
                    "columns": Self::parse_index_columns(&def),
                    "unique":  def.to_ascii_uppercase().contains("UNIQUE INDEX"),
                })
            })
            .collect()
    }

    fn parse_index_columns(def: &str) -> Vec<String> {
        let open = def.rfind('(');
        let close = def.rfind(')');
        match (open, close) {
            (Some(o), Some(c)) if c > o => def[o + 1..c]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn shape_foreign_keys(rows: &[PgRow]) -> Vec<Value> {
        rows.iter()
            .map(|r| {
                let col: String = r.get("column_name");
                let f_schema: String = r.get("foreign_table_schema");
                let f_table: String = r.get("foreign_table_name");
                let f_col: String = r.get("foreign_column_name");
                json!({
                    "columns":    [col],
                    "references": format!("{f_schema}.{f_table}({f_col})"),
                })
            })
            .collect()
    }
}

#[async_trait]
impl Tool for DescTable {
    fn name(&self) -> String {
        "desc_table".to_string()
    }

    fn decription(&self) -> String {
        "Return column, constraint, and index metadata for a Postgres \
         table. Accepts either a bare name like `\"users\"` (defaults to \
         `public.users`) or a dotted name like `\"audit.events\"`."
            .to_string()
    }

    fn parammeters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "description": "Table name. Optional `schema.table` form is accepted."
                }
            },
            "required": ["table"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let (schema, table) = get_schema_table(&args)?;

        let (column_rows, pk_rows, index_rows, fk_rows) = tokio::join!(
            self.fetch_columns(&schema, &table),
            self.fetch_primary_key(&schema, &table),
            self.fetch_indexes(&schema, &table),
            self.fetch_foreign_keys(&schema, &table),
        );

        let col_rows = column_rows?;
        let pk_rows = pk_rows?;
        let idx_rows = index_rows?;
        let fk_rows = fk_rows?;

        let mut columns = DescTable::shape_columns(&col_rows);
        let pk_set = DescTable::shape_primary_key(&pk_rows);

        DescTable::mark_primary_key_columns(&mut columns, pk_set.as_ref());

        let result = json!({
            "table": format!("{schema}.{table}"),
            "columns": columns,
            "primary_key": pk_set,
            "foreign_keys": DescTable::shape_foreign_keys(&fk_rows),
            "indexes": DescTable::shape_indexes(&idx_rows),
        });

        serde_json::to_string(&result)
            .map_err(|e| AgentError::ToolError(format!("desc table error: {e}")))
    }
}
