use anyhow::Result;
use async_trait::async_trait;
use pstore::models::Data;
use reqwest::Client;

#[async_trait]
pub trait RemoteSync: Send + Sync {
    async fn sync(&self, client: &Client, remote_id: i64) -> Result<Data>;
}
