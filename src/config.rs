use crate::source::SERVICE_NAME;
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

pub fn get_password(kind: &str, name: &str) -> Result<String, anyhow::Error> {
    let entry = Entry::new(SERVICE_NAME, &format!("{kind}-{name}"))?;
    Ok(entry.get_password()?)
}

impl SourceConfig {
    fn get_source(&self) -> Result<impl Source> {
        match self {
            SourceConfig::AzureDevops(org) => {
                get_password(&AzureDevops::kind(), org).map(|pat| AzureDevops {
                    org: org.to_owned(),
                    pat,
                })
            }
        }
    }

    pub fn get_filename(&self) -> String {
        match self {
            SourceConfig::AzureDevops(org) => format!("ado-{org}.json"),
        }
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

    pub fn add_source(&mut self, new_source: SourceConfig) -> Result<()> {
        if !self.sources.contains(&new_source) {
            self.sources.push(new_source);
        }
        self.save()?;
        Ok(())
    }

    pub fn remove_source(&mut self, sc_to_remove: &SourceConfig) -> Result<()> {
        self.sources.retain(|s| s != sc_to_remove);
        self.save()?;
        Ok(())
    }
}
