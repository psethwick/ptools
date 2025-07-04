use anyhow::Result;
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use yoink_rs::azure_devops::AzureDevops;
use yoink_rs::config::Config;
use yoink_rs::data::SourceData;
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
}

#[derive(Debug, Subcommand)]
enum Add {
    AzureDevops { org: String, pat: String },
}

#[derive(Debug, Subcommand)]
enum Delete {
    AzureDevops { org: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    match args.command {
        Root::Add(a) => match a {
            Add::AzureDevops { org, pat } => AzureDevops { org, pat }.add()?,
        },
        Root::Delete(d) => match d {
            Delete::AzureDevops { org } => AzureDevops {
                org,
                pat: "".to_owned(),
            }
            .delete()?,
        },
        Root::Sync => {
            let config = Config::load()?;
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
            for d in data {
                d.save()?;
            }
        }
    }

    Ok(())
}
