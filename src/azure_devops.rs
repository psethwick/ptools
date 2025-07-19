use crate::data::{Data, Person, Work};
use crate::source::Source;
use anyhow::{Error, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::task::JoinSet;

async fn get_max_modified(
    pool: &SqlitePool,
    source_id: i64,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let max_modified = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        r#"SELECT MAX(modified) FROM work WHERE source_id = ?"#,
    )
    .bind(source_id)
    .fetch_one(pool)
    .await?;

    Ok(max_modified)
}

pub struct AzureDevops {
    pub org: String,
    pub pat: String,
    pub source_id: i64,
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
    source_id: i64,
    client: &Client,
    org: &str,
    pat: &str,
    project: &Value,
    max_modified: Option<DateTime<Utc>>,
    pool: &SqlitePool,
) -> Result<()> {
    let project_name = project["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Project name not found"))?;

    let project_id = project["id"]
        .as_str()
        .ok_or_else(|| anyhow!("Project id not found"))?;

    let date_filter = if let Some(max_modified_date) = max_modified {
        format!(
            " AND [System.ChangedDate] > '{}'",
            max_modified_date.format("%Y-%m-%dT%H:%M:%S.%3fZ")
        )
    } else {
        "".to_string()
    };

    let wiql_query = format!(
        r#"
        SELECT [System.Id]
        FROM WorkItems
        WHERE [System.TeamProject] = '{project_name}'{date_filter}
        ORDER BY [System.Id]
        "#,
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
        dbg!("no work found");
        return Ok(());
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

    let mut set = JoinSet::new();

    for batch in work_item_ids.chunks(200) {
        let client = client.clone();
        let org = org.to_string();
        let pat = pat.to_string();
        let fields_param = fields_param.clone();
        let batch: Vec<String> = batch.to_vec();

        set.spawn(async move {
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

            let work: Vec<_> = batch_response
                .iter()
                .map(|az| Work {
                    source_id,
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
                .collect();

            let people: Vec<Person> = batch_response
                .iter()
                .flat_map(|c| {
                    [c.fields.created_by.as_ref(), c.fields.assigned_to.as_ref()]
                        .into_iter()
                        .flatten()
                        .map(|p| Person {
                            source_id,
                            id: p.id.clone(),
                            name: p.display_name.clone(),
                        })
                })
                .unique_by(|p| p.id.clone())
                .collect();

            Ok::<Data, Error>(Data { work, people })
        });
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(data)) => {
                let mut tx = pool.begin().await?;
                for work_item in data.work {
                    work_item.save(&mut *tx).await?;
                }
                for person in data.people {
                    person.save(&mut *tx).await?;
                }
                tx.commit().await?;
            }
            Ok(Err(e)) => {
                eprintln!(
                    "Warning: Could not fetch work items batch for project {project_name}: {e}"
                )
            }
            Err(e) => eprintln!("Warning: Batch task failed for project {project_name}: {e}"),
        }
    }

    Ok(())
}

#[async_trait]
impl Source for AzureDevops {
    fn source_id(&self) -> i64 {
        self.source_id
    }

    async fn sync(&self, client: &Client, pool: &SqlitePool) -> Result<(), Error> {
        let max_modified = get_max_modified(pool, self.source_id).await?;

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

        let mut set = JoinSet::new();

        for project in projects {
            let client = client.clone();
            let org = self.org.clone();
            let pat = self.pat.clone();
            let project = project.clone();
            let source_id = self.source_id;
            let pool = pool.clone();

            set.spawn(async move {
                process_project(
                    source_id,
                    &client,
                    &org,
                    &pat,
                    &project,
                    max_modified,
                    &pool,
                )
                .await
            });
        }

        while let Some(res) = set.join_next().await {
            match res {
                Ok(Ok(())) => (),
                Ok(Err(e)) => eprintln!("Project processing error: {e}"),
                Err(e) => eprintln!("Task join error: {e}"),
            }
        }

        Ok(())
    }
}
