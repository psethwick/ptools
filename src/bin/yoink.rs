use anyhow::{Result, anyhow};
use clap::{self, Parser, Subcommand};
use sqlx::{SqlitePool, migrate};
use std::path::PathBuf;
use tokio::task::JoinSet;
use yoink_rs::SERVICE_NAME;
use yoink_rs::source::SourceSync;
use yoink_rs::source::{Source, Work, get_sources, remove_source};

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
    sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await?;
    migrate!("./migrations").run(&pool).await?;

    match args.command {
        Root::Add(a) => match a {
            Add::AzureDevops { org, pat } => {
                let kind = "azure_devops";
                Source::add(&pool, kind, &org, pat).await?;
                println!("Added source: {kind}-{org}");
            }
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => {
                let sc_to_remove = sqlx::query_as::<_, Source>(
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
            let mut set = JoinSet::new();

            for source in sources {
                let client = client.clone();
                let pool = pool.clone();
                set.spawn(async move {
                    if let Err(e) = source.sync(&client, &pool).await {
                        eprintln!("Sync failed: {e}");
                    }
                });
            }

            while let Some(res) = set.join_next().await {
                if let Err(e) = res {
                    eprintln!("Task execution failed: {e}");
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
