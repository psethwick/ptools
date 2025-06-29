use anyhow::Error;
use async_trait::async_trait;
use reqwest::Client;

#[async_trait]
pub trait Source {
    fn kind() -> String;
    fn add(&self) -> anyhow::Result<()>;
    fn delete(&self) -> anyhow::Result<()>;
    async fn sync(self, client: &Client) -> Result<(), Error>;
}
