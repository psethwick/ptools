use anyhow::Error;
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use tokio;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::{CredentialManager, load_config, save_config};
use yoink_rs::source::Source;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let cred_manager = CredentialManager::new("yoink");
    match args.command {
        Root::Add(a) => match a {
            AddCommands::AddAzureDevopsOrg { org, pat } => match cred_manager.store_pat(&org, &pat)
            {
                Ok(()) => {
                    let mut config = load_config()?;
                    match config.sources.get_mut("azure_devops") {
                        Some(ado_orgs) if !ado_orgs.contains(&org) => ado_orgs.push(org),
                        None => {
                            config.sources.insert("azure_devops".to_owned(), vec![org]);
                        }
                        _ => {}
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
                    if let Some(ado_orgs) = config.sources.get_mut("azure_devops") {
                        ado_orgs.retain(|o| *o != org);
                    }
                    save_config(&config)?;
                }
                Err(e) => eprintln!("Failed to delete PAT: {}", e),
            },
        },
        Root::Sync => {
            let config = load_config()?;
            let client = reqwest::Client::new();
            let futures = config
                .sources
                .into_iter()
                .filter_map(|(k, v)| match k {
                    val if val == "azure_devops" => Some(
                        v.into_iter()
                            .filter_map(|org| {
                                cred_manager
                                    .get_pat(&org)
                                    .ok()
                                    .map(|pat| AzureDevops { org, pat }.sync(&client))
                            })
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .flatten();
            let results: Vec<Result<(), Error>> = join_all(futures).await;

            for result in results {
                if let Err(e) = result {
                    eprintln!("Sync failed: {}", e);
                }
            }
        }
    }

    Ok(())
}
