use anyhow::Result;
use clap::{self, Parser, Subcommand};
use pstore::models::Kind;
use pstore::queries::{add_remote, get_remotes, remove_remote, get_work};
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
                add_remote(&pool, Kind::AzureDevops, &org, pat).await?;
                println!("Added remote: {}-{org}", Kind::AzureDevops);
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
                add_remote(&pool, Kind::Jira, &org, credentials.to_string()).await?;
                println!("Added remote: {}-{org}", Kind::Jira);
            }
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => {
                let remotes = get_remotes(&pool).await?;
                let remote_to_remove = remotes.iter().find(|s| s.kind == Kind::AzureDevops && s.name == org).unwrap();
                remove_remote(&pool, remote_to_remove).await?;
            }
            Delete::Jira { .. } => todo!(),
        },
        Root::Sync => {
            let client = reqwest::Client::new();
            let remotes = get_remotes(&pool).await?;
            let mut set = JoinSet::new();

            for remote in remotes {
                let client = client.clone();
                let pool = pool.clone();
                set.spawn(async move {
                    if let Err(e) = psync::sync_remote(&remote, &client, &pool).await {
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
