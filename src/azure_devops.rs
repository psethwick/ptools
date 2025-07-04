use crate::config::{Config, SourceConfig};
use crate::data::{Data, Work};
use crate::source::Source;
use anyhow::{Error, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::task;

pub struct AzureDevops {
    pub org: String,
    pub pat: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AzureDevOpsWorkItemFields {
    #[serde(rename = "System.TeamProject")]
    pub project: String,
    #[serde(rename = "System.Title")]
    pub title: String,
    #[serde(rename = "System.WorkItemType")]
    pub item_type: String,
    #[serde(rename = "System.Description")]
    pub description: Option<String>,
    #[serde(rename = "System.State")]
    pub state: Option<String>,
    #[serde(rename = "System.Parent")]
    pub parent_id: Option<i64>,
    #[serde(rename = "System.BoardColumn")]
    pub column: Option<String>,
    #[serde(rename = "System.CreatedDate")]
    pub created_date: Option<DateTime<Utc>>,
    #[serde(rename = "System.ChangedDate")]
    pub changed_date: Option<DateTime<Utc>>,
    #[serde(rename = "System.CreatedBy")]
    pub created_by: Option<AzureDevOpsPerson>,
    #[serde(rename = "System.AssignedTo")]
    pub assigned_to: Option<AzureDevOpsPerson>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AzureDevOpsPerson {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AzureDevOpsWorkItem {
    pub id: i64,
    pub rev: i64,
    pub url: String,
    pub fields: AzureDevOpsWorkItemFields,
}

#[derive(Deserialize, Debug)]
struct AzureDevOpsBatchResponse {
    pub value: Vec<AzureDevOpsWorkItem>,
}

async fn process_project(
    client: &Client,
    org: &str,
    pat: &str,
    project: &Value,
) -> Result<Vec<Data>> {
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
        WHERE [System.TeamProject] = '{project_name}'
        ORDER BY [System.Id]
        "#
    );

    let wiql_url =
        format!("https://dev.azure.com/{org}/{project_id}/_apis/wit/wiql?api-version=7.1");

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

    let field_names = [
        "System.TeamProject",
        "System.Title",
        "System.WorkItemType",
        "System.Description",
        "System.State",
        "System.Parent",
        "System.BoardColumn",
        "System.CreatedDate",
        "System.ChangedDate",
        "System.CreatedBy",
        "System.AssignedTo",
    ];

    let fields_param = field_names.join(",");

    let batch_tasks: Vec<_> = work_item_ids
        .chunks(200)
        .map(|batch| {
            let client = client.clone();
            let org = org.to_string();
            let pat = pat.to_string();
            let fields_param = fields_param.clone();
            let batch: Vec<String> = batch.to_vec();

            task::spawn(async move {
                let ids_param = batch.join(",");
                let batch_url = format!(
                    "https://dev.azure.com/{org}/_apis/wit/workitems?ids={ids_param}&fields={fields_param}"
                );

                let batch_response = client
                    .get(&batch_url)
                    .bearer_auth(&pat)
                    .send()
                    .await?
                    .json::<AzureDevOpsBatchResponse>()
                    .await?
                    .value;

                // let batch_items = batch_response["value"]
                //     .as_array()
                //     .cloned()
                //     .unwrap_or_else(Vec::new);

                Ok::<Vec<AzureDevOpsWorkItem>, Error>(batch_response)
            })
        })
        .collect();

    let batch_results = join_all(batch_tasks).await;

    let mut items = Vec::new();
    for result in batch_results {
        match result {
            Ok(Ok(batch_items)) => items.extend(batch_items),
            Ok(Err(e)) => eprintln!(
                "Warning: Could not fetch work items batch for project {project_name}: {e}"
            ),
            Err(e) => eprintln!("Warning: Batch task failed for project {project_name}: {e}"),
        }
    }

    Ok(items
        .iter()
        .map(|az| {
            Data::Work(Work {
                source: SourceConfig::AzureDevops(org.to_string()),
                id: az.id.to_string(),
                version: Some(az.rev.to_string()),
                url: Some(az.url.clone()),
                project: az.fields.project.clone(),
                title: az.fields.title.clone(),
                description: az.fields.description.clone(),
                created: az.fields.created_date,
                created_by_id: az.fields.created_by.as_ref().map(|cb| cb.id.to_string()),
                assigned_to_id: az.fields.assigned_to.as_ref().map(|at| at.id.to_string()),
                column: az.fields.column.clone(),
                modified: az.fields.changed_date,
                state: az.fields.state.clone(),
                work_type: az.fields.item_type.clone(),
                parent_id: az.fields.parent_id.map(|pi| pi.to_string()),
            })
        })
        .collect())
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
                let mut config = Config::load()?;
                config.add_source(SourceConfig::AzureDevops(self.org.clone()))?;
                Ok(())
            }
            Err(e) => Err(anyhow!("Failed to store PAT: {}", e)),
        }
    }

    fn delete(&self) -> anyhow::Result<()> {
        match self.delete_password() {
            Ok(()) => {
                let mut config = Config::load()?;
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
                Ok(Err(e)) => eprintln!("Project processing error: {e}"),
                Err(e) => eprintln!("Task join error: {e}"),
            }
        }

        // TODO: get persons?
        Ok(items)
    }
}
