use anyhow::Error;
use async_trait::async_trait;
use reqwest::Client;

#[async_trait]
pub trait Source {
    fn kind() -> String;
    async fn sync(self, client: &Client) -> Result<(), Error>;
}
