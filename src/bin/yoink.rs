use anyhow::{Result, anyhow};
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use sqlx::{SqlitePool, migrate};
use std::path::PathBuf;
use yoink_rs::SERVICE_NAME;
use yoink_rs::data::{SourceConfig, Work, get_sources, remove_source};
use yoink_rs::source::{Source, new_source};

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

    let data_dir = get_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("yoink.db");
    let pool =
        SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
    migrate!("./migrations").run(&pool).await?;

    match args.command {
        Root::Add(a) => match a {
            Add::AzureDevops { org, pat } => {
                let kind = "azure_devops";
                new_source(&pool, kind, &org, pat).await?;
                println!("Added source: {kind}-{org}");
            }
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => {
                let sc_to_remove = sqlx::query_as::<_, SourceConfig>(
                    "SELECT id, kind, name FROM source WHERE kind = ? AND name = ?",
                )
                .bind("azure_devops")
                .bind(org)
                .fetch_one(&pool)
                .await?;
                remove_source(&pool, &sc_to_remove).await?;
            }
        },
        Root::Sync => {
            let client = reqwest::Client::new();
            let sources = get_sources(&pool).await?;
            let mut futures = Vec::new();
            for s in sources {
                let client = client.clone();
                let pool = pool.clone();
                futures.push(tokio::spawn(async move { s.sync(&client, &pool).await }));
            }

            let results = join_all(futures).await;

            for result in results {
                match result {
                    Ok(Ok(())) => (),
                    Ok(Err(e)) => eprintln!("Sync failed: {e}"),
                    Err(e) => eprintln!("Sync task failed: {e}"),
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
