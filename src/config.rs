use crate::{
    azure_devops::AzureDevops,
    source::{Source, get_password},
};
use anyhow::Ok;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceConfig {
    AzureDevops(String), // organisation name
}

pub enum Sources {
    AzureDevops(AzureDevops),
}

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub sources: Vec<SourceConfig>,
}

impl Config {
    pub fn add_source(&mut self, new_source: SourceConfig) -> anyhow::Result<()> {
        if !self.sources.contains(&new_source) {
            self.sources.push(new_source);
        }
        save_config(self)?;
        anyhow::Ok(())
    }

    pub fn remove_source(&mut self, sc_to_remove: &SourceConfig) -> anyhow::Result<()> {
        self.sources.retain(|s| s != sc_to_remove);
        save_config(self)?;
        anyhow::Ok(())
    }
}

pub fn from_config(sc: SourceConfig) -> Result<impl Source> {
    match sc {
        SourceConfig::AzureDevops(org) => {
            get_password(&AzureDevops::kind(), &org).map(|pat| AzureDevops { org, pat })
        }
    }
}

fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut path| {
        path.push("yoink");
        path.push("config.toml");
        path
    })
}

pub fn load_config() -> Result<Config, anyhow::Error> {
    if let Some(config_path) = get_config_path() {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }
    Ok(Config::default())
}

pub fn save_config(config: &Config) -> Result<(), anyhow::Error> {
    if let Some(config_path) = get_config_path() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(config)?;
        fs::write(config_path, content)?;
    }
    Ok(())
}
