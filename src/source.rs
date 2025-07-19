use anyhow::{Ok, Result};
use async_trait::async_trait;
use reqwest::Client;
use sqlx::SqlitePool;

use crate::data::SourceConfig;

pub async fn new_source(pool: &SqlitePool, kind: &str, name: &str, password: String) -> Result<()> {
    let result = sqlx::query("INSERT OR REPLACE INTO source (kind, name) VALUES (?, ?)")
        .bind(kind)
        .bind(name)
        .execute(pool)
        .await?;

    let id = result.last_insert_rowid();
    let sc = SourceConfig {
        id,
        kind: kind.to_owned(),
        name: name.to_owned(),
    };

    sc.store_password(&password)?;
    Ok(())
}

#[async_trait]
pub trait Source: Send + Sync {
    fn source_id(&self) -> i64;

    async fn sync(&self, client: &Client, pool: &SqlitePool) -> Result<()>;
}
