use anyhow::Error;
use async_trait::async_trait;
use keyring::Entry;
use reqwest::Client;
use std::path::PathBuf;

const SERVICE_NAME: &'static str = "yoink";

#[async_trait]
pub trait Source {
    fn kind() -> String;
    fn name(&self) -> String;
    fn add(&self) -> anyhow::Result<()>;
    fn delete(&self) -> anyhow::Result<()>;
    async fn sync(self, client: &Client) -> Result<(), Error>;

    fn get_data_path(&self, name: &str) -> Option<PathBuf> {
        dirs::data_dir().map(|mut path| {
            path.push("yoink");
            path.push(Self::kind());
            path.push(self.name());
            path.push(name);
            path
        })
    }

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
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", kind, name))?;
    Ok(entry.get_password()?)
}
