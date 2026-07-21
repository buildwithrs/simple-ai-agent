use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

// search_schema
pub struct SearchSchema {
    pub conn: PgPool,
}

pub struct ExecuteQuery {
    pub conn: PgPool,
}

pub struct ExecuteDML {
    pub conn: PgPool,
}

// DDL
pub struct ExecuteDDL {
    pub conn: PgPool,
}

