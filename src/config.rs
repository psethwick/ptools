use crate::SERVICE_NAME;
use crate::{azure_devops::AzureDevops, source::Source};
use anyhow::Result;
use anyhow::{Error, Ok};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SourceConfig {
    AzureDevops(String), // organisation name
}

impl SourceConfig {
    fn kind(&self) -> String {
        match self {
            SourceConfig::AzureDevops(_) => "azure_devops".to_owned(),
        }
    }

    fn name(&self) -> String {
        match self {
            SourceConfig::AzureDevops(org) => org.to_owned(),
        }
    }

    fn get_source(&self) -> Result<impl Source> {
        match self {
            SourceConfig::AzureDevops(org) => self.get_password().map(|pat| AzureDevops {
                org: org.to_owned(),
                pat,
            }),
        }
    }

    pub fn get_filename(&self) -> String {
        format!("{}-{}", self.kind(), self.name())
    }

    fn get_password(&self) -> Result<String, anyhow::Error> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind(), self.name()))?;
        Ok(entry.get_password()?)
    }

    pub fn store_password(&self, password: &str) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind(), self.name()))?;
        entry.set_password(password)?;
        Ok(())
    }

    pub fn delete_password(&self) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind(), self.name()))?;
        Ok(entry.delete_credential()?)
    }
}

pub enum Sources {
    AzureDevops(AzureDevops),
}

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    sources: Vec<SourceConfig>,
}

fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut path| {
        path.push(SERVICE_NAME);
        path.push("config.toml");
        path
    })
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        if let Some(config_path) = get_config_path() {
            if config_path.exists() {
                let content = fs::read_to_string(config_path)?;
                let config: Config = toml::from_str(&content)?;
                return Ok(config);
            }
        }
        Ok(Config::default())
    }

    fn save(&self) -> Result<(), anyhow::Error> {
        if let Some(config_path) = get_config_path() {
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(self)?;
            fs::write(config_path, content)?;
        }
        Ok(())
    }

    pub fn sources(&self) -> Vec<impl Source> {
        self.sources.iter().flat_map(|s| s.get_source()).collect()
    }

    pub fn add_source(&mut self, new_source: impl Source) -> Result<()> {
        new_source.add()?;
        let source_config = new_source.source_config();
        if !self.sources.contains(&source_config) {
            self.sources.push(source_config);
        }
        self.save()?;
        Ok(())
    }

    pub fn remove_source(&mut self, sc_to_remove: &SourceConfig) -> Result<()> {
        self.sources.retain(|s| s != sc_to_remove);
        self.save()?;
        sc_to_remove.delete_password()?;
        Ok(())
    }
}
