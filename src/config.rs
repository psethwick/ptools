use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub sources: HashMap<String, Vec<String>>,
}

pub struct CredentialManager {
    service_name: &'static str,
}

// TODO: I'm not sure this justifies its existence
impl CredentialManager {
    pub fn new(service_name: &'static str) -> Self {
        Self { service_name }
    }

    pub fn store_pat(&self, org: &str, pat: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.set_password(pat)
    }

    pub fn get_pat(&self, org: &str) -> Result<String, keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.get_password()
    }

    pub fn delete_pat(&self, org: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.delete_credential()
    }
}

fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut path| {
        path.push("yoink");
        path.push("config.toml");
        path
    })
}

pub fn get_data_path(kind: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(kind);
        path.push(name);
        path
    })
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }
    Ok(Config::default())
}

pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(config)?;
        fs::write(config_path, content)?;
    }
    Ok(())
}
