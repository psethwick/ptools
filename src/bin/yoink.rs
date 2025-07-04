use std::fs::File;

use anyhow::Result;
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::Config;
use yoink_rs::data::Data;
use yoink_rs::source::Source;
use yoink_rs::storage::get_data_path;

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
            let config = Config::load()?;
            let client = reqwest::Client::new();
            let futures = config.sources().into_iter().map(|s| s.sync(&client));

            let results: Vec<Result<Vec<Data>>> = join_all(futures).await;
            let data: Vec<Data> = results
                .into_iter()
                .flat_map(|rvd| match rvd {
                    Ok(vd) => Some(vd),
                    Err(e) => {
                        eprintln!("Sync failed: {e}");
                        None
                    }
                })
                .flatten()
                .collect();

            if let Some(path) = get_data_path("data.json") {
                if let Some(parent_dir) = path.parent() {
                    std::fs::create_dir_all(parent_dir)?;
                }

                let file = File::create(&path)?;
                serde_json::to_writer_pretty(file, &data)?;
                println!("Successfully serialized items to {path:?}");
            }
        }
    }

    Ok(())
}
