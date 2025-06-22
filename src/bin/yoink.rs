use clap::{self, Parser, Subcommand};
use keyring::Entry;
use reqwest::{Client, Request};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

trait Source {
    fn kind() -> String;
    fn sync(&self, _: Client); // TODO: this will eventually take local edits
}

struct AzureDevops {
    org: String,
    pat: String,
}

impl Source for AzureDevops {
    fn kind() -> String {
        "AzureDevops".to_owned()
    }

    fn sync(&self, client: Client) {}
}

#[derive(Debug, Parser)]
#[command(name = "yoink")]
#[command(about = "That data, it's mine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    AddAzureDevopsOrg {
        org: String,
        pat: String,
    },
    DeleteAzureDevopsOrg {
        org: String,
    },
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    azure_devops_orgs: Vec<String>,
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
        Commands::AddAzureDevopsOrg { org, pat } => {
            match cred_manager.store_pat(&org, &pat) {
                Ok(()) => {
                    let mut config = load_config()?;
                    config.azure_devops_orgs.push(org.to_string());
                    save_config(&config)?;
                    println!("PAT stored securely")
                }
                Err(e) => eprintln!("Failed to store PAT: {}", e),
            };
        }
        Commands::DeleteAzureDevopsOrg { org } => match cred_manager.delete_pat(&org) {
            Ok(()) => {
                let mut config = load_config()?;
                config.azure_devops_orgs.retain(|o| *o != org);
                save_config(&config)?;
            }
            Err(e) => eprintln!("Failed to delete PAT: {}", e),
        },
    }

    println!("Config saved to: {:?}", get_config_path());

    Ok(())
}
