use crate::{config::SourceConfig, data::SourceData};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

pub const SERVICE_NAME: &str = "yoink";

#[async_trait]
pub trait Source {
    fn source_config(&self) -> SourceConfig;
    fn add(&self) -> Result<()>;
    async fn sync(self, client: &Client) -> Result<SourceData>;
}
