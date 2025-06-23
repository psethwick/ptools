use async_trait::async_trait;
use clap::{self, Parser, Subcommand};
use keyring::Entry;
use reqwest::{Client, Request};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[async_trait]
trait Source {
    fn kind() -> String;
    async fn sync(&self, client: &Client) -> Result<(), Box<dyn std::error::Error>>;
}

struct AzureDevops {
    org: String,
    pat: String,
}

#[async_trait]
impl Source for AzureDevops {
    fn kind() -> String {
        "AzureDevops".to_owned()
    }

    async fn sync(&self, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[derive(Debug, Parser)]
#[command(name = "yoink")]
#[command(about = "That data, it's mine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Root,
}

#[derive(Debug, Subcommand)]
enum Root {
    #[command(subcommand)]
    Add(AddCommands),
    #[command(subcommand)]
    Delete(DeleteCommands),
    Sync,
}

#[derive(Debug, Subcommand)]
enum AddCommands {
    AddAzureDevopsOrg { org: String, pat: String },
}

#[derive(Debug, Subcommand)]
enum DeleteCommands {
    DeleteAzureDevopsOrg { org: String },
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    sources: HashMap<String, Vec<String>>,
}

struct CredentialManager {
    service_name: &'static str,
}

impl CredentialManager {
    fn new(service_name: &'static str) -> Self {
        Self { service_name }
    }

    fn store_pat(&self, org: &str, pat: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.set_password(pat)
    }

    fn get_pat(&self, org: &str) -> Result<String, keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.get_password()
    }

    fn delete_pat(&self, org: &str) -> Result<(), keyring::Error> {
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

fn get_data_path(kind: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(kind);
        path.push(name);
        path
    })
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }
    Ok(Config::default())
}

fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(config)?;
        fs::write(config_path, content)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let cred_manager = CredentialManager::new("yoink");

    match args.command {
        Root::Add(a) => match a {
            AddCommands::AddAzureDevopsOrg { org, pat } => match cred_manager.store_pat(&org, &pat)
            {
                Ok(()) => {
                    let mut config = load_config()?;
                    if let Some(ado_orgs) = config.sources.get_mut("ado") {
                        ado_orgs.push(org);
                    }
                    save_config(&config)?;
                    println!("PAT stored securely")
                }
                Err(e) => eprintln!("Failed to store PAT: {}", e),
            },
        },
        Root::Delete(d) => match d {
            DeleteCommands::DeleteAzureDevopsOrg { org } => match cred_manager.delete_pat(&org) {
                Ok(()) => {
                    let mut config = load_config()?;
                    if let Some(ado_orgs) = config.sources.get_mut("ado") {
                        ado_orgs.retain(|o| *o != org);
                    }
                    save_config(&config)?;
                }
                Err(e) => eprintln!("Failed to delete PAT: {}", e),
            },
        },
        Root::Sync => todo!(),
    }

    println!("Config saved to: {:?}", get_config_path());

    Ok(())
}
