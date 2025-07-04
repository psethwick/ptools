use crate::{config::SourceConfig, data::Data};
use anyhow::Error;
use async_trait::async_trait;
use keyring::Entry;
use reqwest::Client;

const SERVICE_NAME: &str = "yoink";

#[async_trait]
pub trait Source {
    fn kind() -> String;
    fn source_config(&self) -> SourceConfig;
    fn name(&self) -> String;
    fn add(&self) -> anyhow::Result<()>;
    fn delete(&self) -> anyhow::Result<()>;
    async fn sync(self, client: &Client) -> Result<Vec<Data>, Error>;

    fn store_password(&self, password: &str) -> Result<(), anyhow::Error> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", Self::kind(), self.name()))?;
        entry.set_password(password)?;
        Ok(())
    }

    fn delete_password(&self) -> Result<(), anyhow::Error> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", Self::kind(), self.name()))?;
        Ok(entry.delete_credential()?)
    }
}

pub fn get_password(kind: &str, name: &str) -> Result<String, anyhow::Error> {
    let entry = Entry::new(SERVICE_NAME, &format!("{kind}-{name}"))?;
    Ok(entry.get_password()?)
}
