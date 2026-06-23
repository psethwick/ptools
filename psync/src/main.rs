use anyhow::Result;
use clap::{Parser, Subcommand};
use pstore::models::Kind;
use pstore::queries::{add_remote, get_remotes, get_work, get_releases, remove_remote, set_password};
use tokio::task::JoinSet;

#[derive(Debug, Parser)]
#[command(name = "psync")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Sync work items AND releases from all remotes
    Pull,
    /// Push time entries to all remotes
    Push,
    #[command(subcommand)]
    List(ListCommand),
    /// Add a remote
    #[command(subcommand)]
    Add(AddCommand),
    /// Delete a remote
    #[command(subcommand)]
    Delete(DeleteCommand),
    /// Update a remote's secret
    #[command(subcommand)]
    Set(SetCommand),
}

#[derive(Debug, Subcommand)]
enum ListCommand {
    /// List stored work items (default)
    Work,
    /// List stored releases
    Releases,
}

#[derive(Debug, Subcommand)]
enum AddCommand {
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
enum DeleteCommand {
    AzureDevops { org: String },
    Jira { org: String },
}

#[derive(Debug, Subcommand)]
enum SetCommand {
    AzureDevops { org: String, pat: String },
    Jira { org: String, user: String, password: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let pool = pstore::db::init().await?;

    match args.command {
        Command::Pull => {
            let client = reqwest::Client::new();
            let remotes = get_remotes(&pool).await?;
            let mut set = JoinSet::new();

            for remote in remotes {
                let client = client.clone();
                let pool = pool.clone();
                let remote_name = remote.name.clone();
                let remote_kind = remote.kind;
                set.spawn(async move {
                    if let Err(e) = psync::pull(&remote, &client, &pool).await {
                        eprintln!("Sync failed for {remote_kind}-{remote_name}: {e}");
                    }
                });
            }

            while let Some(res) = set.join_next().await {
                if let Err(e) = res {
                    eprintln!("Task execution failed: {e}");
                }
            }
        }
        Command::Push => {
            let client = reqwest::Client::new();
            let remotes = get_remotes(&pool).await?;
            let mut set = JoinSet::new();

            for remote in remotes {
                let client = client.clone();
                let pool = pool.clone();
                set.spawn(async move {
                    if let Err(e) = psync::push_time(&remote, &client, &pool).await {
                        eprintln!("Push failed for {}: {e}", remote.name);
                    }
                });
            }

            while let Some(res) = set.join_next().await {
                if let Err(e) = res {
                    eprintln!("Task execution failed: {e}");
                }
            }
        }
        Command::List(list_cmd) => match list_cmd {
            ListCommand::Work => {
                let work_items = get_work(&pool).await?;
                for item in work_items {
                    println!("{item:#?}");
                }
            }
            ListCommand::Releases => {
                let releases = get_releases(&pool).await?;
                for release in releases {
                    println!("{release:#?}");
                }
            }
        },
        Command::Add(add_cmd) => match add_cmd {
            AddCommand::AzureDevops { org, pat } => {
                add_remote(&pool, Kind::AzureDevops, &org, pat).await?;
                println!("Added remote: {}-{org}", Kind::AzureDevops);
            }
            AddCommand::Jira {
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
        Command::Delete(delete_cmd) => match delete_cmd {
            DeleteCommand::AzureDevops { org } => {
                let remotes = get_remotes(&pool).await?;
                let remote_to_remove = remotes
                    .iter()
                    .find(|s| s.kind == Kind::AzureDevops && s.name == org)
                    .unwrap();
                remove_remote(&pool, remote_to_remove).await?;
                println!("Deleted remote: {}-{org}", Kind::AzureDevops);
            }
            DeleteCommand::Jira { org } => {
                let remotes = get_remotes(&pool).await?;
                let remote_to_remove = remotes
                    .iter()
                    .find(|s| s.kind == Kind::Jira && s.name == org)
                    .unwrap();
                remove_remote(&pool, remote_to_remove).await?;
                println!("Deleted remote: {}-{org}", Kind::Jira);
            }
        },
        Command::Set(set_cmd) => match set_cmd {
            SetCommand::AzureDevops { org, pat } => {
                let remotes = get_remotes(&pool).await?;
                let remote = remotes
                    .iter()
                    .find(|s| s.kind == Kind::AzureDevops && s.name == org)
                    .ok_or_else(|| anyhow::anyhow!("Remote not found: {}-{}", Kind::AzureDevops, org))?;
                set_password(remote, &pat)?;
                println!("Updated secret for: {}-{org}", Kind::AzureDevops);
            }
            SetCommand::Jira { org, user, password } => {
                let remotes = get_remotes(&pool).await?;
                let remote = remotes
                    .iter()
                    .find(|s| s.kind == Kind::Jira && s.name == org)
                    .ok_or_else(|| anyhow::anyhow!("Remote not found: {}-{}", Kind::Jira, org))?;
                let credentials = serde_json::json!({
                    "user": user,
                    "password": password
                });
                set_password(remote, &credentials.to_string())?;
                println!("Updated secret for: {}-{org}", Kind::Jira);
            }
        },
    }

    Ok(())
}
