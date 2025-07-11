use anyhow::{anyhow, Result};
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use sqlx::{migrate, SqlitePool};
use std::path::PathBuf;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::{Config, SourceConfig};
use yoink_rs::data::{SourceData, Work};
use yoink_rs::source::{Source, SERVICE_NAME};

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
    let pool = SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
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
            let futures = config.sources().into_iter().map(|s| s.sync(&client));

            let results: Vec<Result<SourceData>> = join_all(futures).await;
            let data: Vec<SourceData> = results
                .into_iter()
                .flat_map(|rvd| match rvd {
                    Ok(vd) => Some(vd),
                    Err(e) => {
                        eprintln!("Sync failed: {e}");
                        None
                    }
                })
                .collect();

            let mut tx = pool.begin().await?;
            for d in data {
                for work_item in d.work {
                    sqlx::query(
                        "INSERT OR REPLACE INTO work (project, id, title, parent_id, description, work_type, version, state, created_by_id, assigned_to_id, column, created, modified, url)
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(work_item.project)
                    .bind(work_item.id)
                    .bind(work_item.title)
                    .bind(work_item.parent_id)
                    .bind(work_item.description)
                    .bind(work_item.work_type)
                    .bind(work_item.version)
                    .bind(work_item.state)
                    .bind(work_item.created_by_id)
                    .bind(work_item.assigned_to_id)
                    .bind(work_item.column)
                    .bind(work_item.created)
                    .bind(work_item.modified)
                    .bind(work_item.url)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            tx.commit().await?;
        }
        Root::List(l) => match l {
            List::Work => {
                let work_items: Vec<Work> = sqlx::query_as("SELECT * FROM work").fetch_all(&pool).await?;
                for item in work_items {
                    println!("{:#?}", item);
                }
            }
        },
    }

    Ok(())
}