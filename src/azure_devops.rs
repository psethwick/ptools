use crate::config::{SourceConfig, load_config};
use crate::data::Data;
use crate::source::Source;
use anyhow::{Error, Result, anyhow};
use async_trait::async_trait;
use futures::future::join_all;
use reqwest::Client;
use serde_json::{Value, json};
use std::fs::File;
use tokio::task;

pub struct AzureDevops {
    pub org: String,
    pub pat: String,
}

async fn process_project(
    client: &Client,
    org: &str,
    pat: &str,
    project: &Value,
) -> Result<Vec<Value>> {
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
    // TODO: I wonder if kind() is necessary
    // or name()
    fn kind() -> String {
        "azure_devops".to_owned()
    }

    fn source_config(&self) -> SourceConfig {
        SourceConfig::AzureDevops(self.org.clone())
    }

    fn name(&self) -> String {
        self.org.clone()
    }

    fn add(&self) -> anyhow::Result<()> {
        match self.store_password(&self.pat) {
            Ok(()) => {
                let mut config = load_config()?;
                config.add_source(SourceConfig::AzureDevops(self.org.clone()))?;
                Ok(())
            }
            Err(e) => Err(anyhow!("Failed to store PAT: {}", e)),
        }
    }

    fn delete(&self) -> anyhow::Result<()> {
        match self.delete_password() {
            Ok(()) => {
                let mut config = load_config()?;
                config.remove_source(&SourceConfig::AzureDevops(self.org.clone()))?;
                Ok(())
            }
            Err(e) => Err(anyhow!("Failed to delete PAT: {}", e)),
        }
    }

    async fn sync(self, client: &Client) -> Result<Vec<Data>, Error> {
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

        if let Some(path) = self.get_data_path("work.json") {
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
