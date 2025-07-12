use crate::config::SourceConfig;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use sqlx::SqlitePool;

#[async_trait]
pub trait Source {
    fn source_config(&self) -> SourceConfig;
    fn add(&self) -> Result<()>;
    async fn sync(self, client: &Client, pool: &SqlitePool) -> Result<()>;
}
