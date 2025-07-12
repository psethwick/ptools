use crate::config::SourceConfig;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use sqlx::{Pool, Sqlite};

#[async_trait]
pub trait Source {
    fn source_config(&self) -> SourceConfig;
    fn add(&self) -> Result<()>;
    async fn sync(self, client: &Client, pool: Pool<Sqlite>) -> Result<()>;
}
