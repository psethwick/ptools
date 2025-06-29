use anyhow::Ok;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub sources: HashMap<String, Vec<String>>,
}

const SERVICE_NAME: &'static str = "yoink";

pub fn store_password(kind: &str, name: &str, password: &str) -> Result<(), anyhow::Error> {
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", kind, name))?;
    entry.set_password(password)?;
    Ok(())
}

pub fn get_password(kind: &str, name: &str) -> Result<String, anyhow::Error> {
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", kind, name))?;
    Ok(entry.get_password()?)
}

pub fn delete_password(kind: &str, org: &str) -> Result<(), anyhow::Error> {
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", kind, org))?;
    Ok(entry.delete_credential()?)
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
