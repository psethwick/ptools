use anyhow::{Result, anyhow};
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use sqlx::{SqlitePool, migrate};
use std::path::PathBuf;
use yoink_rs::SERVICE_NAME;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::{Config, SourceConfig};
use yoink_rs::data::Work;
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
    Add(Add),
    #[command(subcommand)]
    Delete(Delete),
    Sync,
    #[command(subcommand)]
    List(List),
}

#[derive(Debug, Subcommand)]
enum List {
    Work,
}

#[derive(Debug, Subcommand)]
enum Add {
    AzureDevops { org: String, pat: String },
}

#[derive(Debug, Subcommand)]
enum Delete {
    AzureDevops { org: String },
}

fn get_data_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|mut path| {
            path.push(SERVICE_NAME);
            path
        })
        .ok_or(anyhow!("Couldn't determine data directory"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let mut config = Config::load()?;

    let data_dir = get_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("yoink.db");
    let pool =
        SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
    migrate!("./migrations").run(&pool).await?;

    match args.command {
        Root::Add(a) => match a {
            Add::AzureDevops { org, pat } => config.add_source(AzureDevops { org, pat })?,
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => config.remove_source(&SourceConfig::AzureDevops(org))?,
        },
        Root::Sync => {
            let client = reqwest::Client::new();
            let futures = config.sources().into_iter().map(|s| s.sync(&client, &pool));

            let results: Vec<Result<()>> = join_all(futures).await;

            for result in results {
                if let Err(e) = result {
                    eprintln!("Sync failed: {e}");
                }
            }
        }
        Root::List(l) => match l {
            List::Work => {
                let work_items: Vec<Work> = sqlx::query_as("SELECT * FROM work")
                    .fetch_all(&pool)
                    .await?;
                for item in work_items {
                    println!("{item:#?}");
                }
            }
        },
    }

    Ok(())
}
