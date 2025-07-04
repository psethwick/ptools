use crate::{
    azure_devops::AzureDevops,
    source::{Source, get_password},
};
use anyhow::Result;
use anyhow::{Error, Ok};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SourceConfig {
    AzureDevops(String), // organisation name
}

pub enum Sources {
    AzureDevops(AzureDevops),
}

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    sources: Vec<SourceConfig>,
}

fn source_from_config(sc: &SourceConfig) -> Result<impl Source> {
    match sc {
        SourceConfig::AzureDevops(org) => {
            get_password(&AzureDevops::kind(), org).map(|pat| AzureDevops {
                org: org.to_owned(),
                pat,
            })
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

    pub fn save(&self) -> Result<(), anyhow::Error> {
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
        self.sources
            .iter()
            .flat_map(|s| source_from_config(s))
            .collect()
    }

    pub fn add_source(&mut self, new_source: SourceConfig) -> anyhow::Result<()> {
        if !self.sources.contains(&new_source) {
            self.sources.push(new_source);
        }
        self.save()?;
        anyhow::Ok(())
    }

    pub fn remove_source(&mut self, sc_to_remove: &SourceConfig) -> anyhow::Result<()> {
        self.sources.retain(|s| s != sc_to_remove);
        self.save()?;
        anyhow::Ok(())
    }
}
