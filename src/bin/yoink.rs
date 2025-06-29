use anyhow::Error;
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use tokio;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::{get_password, load_config};
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
    AzureDevopsOrg { org: String, pat: String },
}

#[derive(Debug, Subcommand)]
enum DeleteCommands {
    AzureDevopsOrg { org: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    match args.command {
        Root::Add(a) => match a {
            AddCommands::AzureDevopsOrg { org, pat } => AzureDevops { org, pat }.add()?,
        },
        Root::Delete(d) => match d {
            DeleteCommands::AzureDevopsOrg { org } => AzureDevops {
                org,
                pat: "".to_owned(),
            }
            .delete()?,
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
                                get_password("azure_devops", &org)
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
