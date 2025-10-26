use anyhow::Result;
use clap::{self, Parser, Subcommand};
use pstore::models::{Kind, Source, Work};
use pstore::queries::{add_source, get_sources, remove_source, get_work};
use tokio::task::JoinSet;

#[derive(Debug, Parser)]
#[command(name = "psync")]
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
    AzureDevops {
        org: String,
        pat: String,
    },
    Jira {
        org: String,
        user: String,
        password: String,
    },
}

#[derive(Debug, Subcommand)]
enum Delete {
    AzureDevops { org: String },
    Jira { org: String },
}


#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let pool = pstore::db::init().await?;

    match args.command {
        Root::Add(a) => match a {
            Add::AzureDevops { org, pat } => {
                add_source(&pool, Kind::AzureDevops, &org, pat).await?;
                println!("Added source: {}-{org}", Kind::AzureDevops);
            }
            Add::Jira {
                org,
                user,
                password,
            } => {
                let credentials = serde_json::json!({
                    "user": user,
                    "password": password
                });
                add_source(&pool, Kind::Jira, &org, credentials.to_string()).await?;
                println!("Added source: {}-{org}", Kind::Jira);
            }
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => {
                let sources = get_sources(&pool).await?;
                let sc_to_remove = sources.iter().find(|s| s.kind == Kind::AzureDevops && s.name == org).unwrap();
                remove_source(&pool, sc_to_remove).await?;
            }
            Delete::Jira { .. } => todo!(),
        },
        Root::Sync => {
            let client = reqwest::Client::new();
            let sources = get_sources(&pool).await?;
            let mut set = JoinSet::new();

            for source in sources {
                let client = client.clone();
                let pool = pool.clone();
                set.spawn(async move {
                    if let Err(e) = psync::sync_source(&source, &client, &pool).await {
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
                let work_items = get_work(&pool).await?;
                for item in work_items {
                    println!("{item:#?}");
                }
            }
        },
    }

    Ok(())
}
