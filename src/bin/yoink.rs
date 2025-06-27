use anyhow::{Error, anyhow};
use async_trait::async_trait;
use clap::{self, Parser, Subcommand};
use futures::future::join_all;
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::format;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use tokio::task;

#[async_trait]
trait Source {
    fn kind() -> String;
    async fn sync(self, client: &Client) -> Result<(), Error>;
}

struct AzureDevops {
    org: String,
    pat: String,
}

async fn process_project(
    client: &Client,
    org: &str,
    pat: &str,
    project: &Value,
) -> Result<Vec<Value>, Error> {
    let project_name = project["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Project name not found"))?;

    let project_id = project["id"]
        .as_str()
        .ok_or_else(|| anyhow!("Project id not found"))?;

    let wiql_query = format!(
        r#"
        SELECT [System.Id]
        FROM WorkItems
        WHERE [System.TeamProject] = '{}'
        ORDER BY [System.Id]
        "#,
        project_name
    );

    let wiql_url = format!(
        "https://dev.azure.com/{}/{}/_apis/wit/wiql?api-version=7.1",
        org, project_id
    );

    let wiql_body = json!({
        "query": wiql_query
    });

    let wiql_response = client
        .post(&wiql_url)
        .bearer_auth(pat)
        .json(&wiql_body)
        .send()
        .await?
        .json::<Value>()
        .await?;

    let work_item_ids: Vec<String> = wiql_response
        .get("workItems")
        .and_then(|wi| wi.as_array())
        .map(|work_items| {
            work_items
                .iter()
                .filter_map(|wi| wi["id"].as_u64())
                .map(|id| id.to_string())
                .collect()
        })
        .unwrap_or_else(Vec::new);

    if work_item_ids.is_empty() {
        return Ok(Vec::new());
    }

    // let field_names = [
    //     "System.AssignedTo",
    //     "System.BoardColumn",
    //     "System.TeamProject",
    //     "System.ChangedDate",
    //     "Microsoft.VSTS.Common.ActivatedDate",
    //     "System.Description",
    //     "System.Title",
    //     "System.WorkItemType",
    // ];
    // let fields_param = field_names.join(",");

    let batch_tasks: Vec<_> = work_item_ids
        .chunks(200)
        .map(|batch| {
            let client = client.clone();
            let org = org.to_string();
            let pat = pat.to_string();
            // let fields_param = fields_param.clone();
            let batch: Vec<String> = batch.to_vec();

            task::spawn(async move {
                let ids_param = batch.join(",");
                let batch_url = format!(
                    "https://dev.azure.com/{}/_apis/wit/workitems?ids={}&$expand=All",
                    // TODO: probably put back the explicit fields once I know what I want to use
                    // and change $expand to only 'relations'
                    org,
                    ids_param
                );

                let batch_response = client
                    .get(&batch_url)
                    .bearer_auth(&pat)
                    .send()
                    .await?
                    .json::<Value>()
                    .await?;

                let batch_items = batch_response["value"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(Vec::new);

                Ok::<Vec<Value>, Error>(batch_items)
            })
        })
        .collect();

    let batch_results = join_all(batch_tasks).await;

    let mut items = Vec::new();
    for result in batch_results {
        match result {
            Ok(Ok(batch_items)) => items.extend(batch_items),
            Ok(Err(e)) => eprintln!(
                "Warning: Could not fetch work items batch for project {}: {}",
                project_name, e
            ),
            Err(e) => eprintln!(
                "Warning: Batch task failed for project {}: {}",
                project_name, e
            ),
        }
    }

    Ok(items)
}

#[async_trait]
impl Source for AzureDevops {
    fn kind() -> String {
        "AzureDevops".to_owned()
    }

    async fn sync(self, client: &Client) -> Result<(), Error> {
        let projects_url = format!(
            "https://dev.azure.com/{}/_apis/projects?api-version=7.1",
            self.org
        );
        let projects_response: Value = client
            .get(&projects_url)
            .bearer_auth(&self.pat)
            .send()
            .await?
            .json()
            .await?;

        let empty_projects = Vec::new();
        let projects = projects_response["value"]
            .as_array()
            .unwrap_or(&empty_projects);

        let project_tasks: Vec<_> = projects
            .iter()
            .map(|project| {
                let client = client.clone();
                let org = self.org.clone();
                let pat = self.pat.clone();
                let project = project.clone();

                task::spawn(async move { process_project(&client, &org, &pat, &project).await })
            })
            .collect();

        let project_results = join_all(project_tasks).await;

        let mut items = Vec::new();
        for result in project_results {
            match result {
                Ok(Ok(project_items)) => items.extend(project_items),
                Ok(Err(e)) => eprintln!("Project processing error: {}", e),
                Err(e) => eprintln!("Task join error: {}", e),
            }
        }

        if let Some(path) = get_data_path("ado", &format!("{}.json", &self.org)) {
            if let Some(parent_dir) = path.parent() {
                std::fs::create_dir_all(parent_dir)?;
            }

            let file = File::create(&path)?;
            serde_json::to_writer_pretty(file, &items)?;

            println!("Successfully serialized items to {:?}", path);
        }
        Ok(())
    }
}

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
    AddAzureDevopsOrg { org: String, pat: String },
}

#[derive(Debug, Subcommand)]
enum DeleteCommands {
    DeleteAzureDevopsOrg { org: String },
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    sources: HashMap<String, Vec<String>>,
}

struct CredentialManager {
    service_name: &'static str,
}

impl CredentialManager {
    fn new(service_name: &'static str) -> Self {
        Self { service_name }
    }

    fn store_pat(&self, org: &str, pat: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.set_password(pat)
    }

    fn get_pat(&self, org: &str) -> Result<String, keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.get_password()
    }

    fn delete_pat(&self, org: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new(self.service_name, org)?;
        entry.delete_credential()
    }
}

fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut path| {
        path.push("yoink");
        path.push("config.toml");
        path
    })
}

fn get_data_path(kind: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(kind);
        path.push(name);
        path
    })
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }
    Ok(Config::default())
}

fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(config_path) = get_config_path() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(config)?;
        fs::write(config_path, content)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let cred_manager = CredentialManager::new("yoink");
    match args.command {
        Root::Add(a) => match a {
            AddCommands::AddAzureDevopsOrg { org, pat } => match cred_manager.store_pat(&org, &pat)
            {
                Ok(()) => {
                    let mut config = load_config()?;
                    match config.sources.get_mut("ado") {
                        Some(ado_orgs) if !ado_orgs.contains(&org) => ado_orgs.push(org),
                        None => {
                            config.sources.insert("ado".to_owned(), vec![org]);
                        }
                        _ => {}
                    }
                    save_config(&config)?;
                    println!("PAT stored securely")
                }
                Err(e) => eprintln!("Failed to store PAT: {}", e),
            },
        },
        Root::Delete(d) => match d {
            DeleteCommands::DeleteAzureDevopsOrg { org } => match cred_manager.delete_pat(&org) {
                Ok(()) => {
                    let mut config = load_config()?;
                    if let Some(ado_orgs) = config.sources.get_mut("ado") {
                        ado_orgs.retain(|o| *o != org);
                    }
                    save_config(&config)?;
                }
                Err(e) => eprintln!("Failed to delete PAT: {}", e),
            },
        },
        Root::Sync => {
            let config = load_config()?;
            let client = reqwest::Client::new();
            let futures = config
                .sources
                .into_iter()
                .filter_map(|(k, v)| match k {
                    val if val == "ado" => Some(
                        v.into_iter()
                            .filter_map(|org| {
                                cred_manager
                                    .get_pat(&org)
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
