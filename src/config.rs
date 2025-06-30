use anyhow::Ok;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub sources: HashMap<String, Vec<String>>,
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
