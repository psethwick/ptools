use crate::remote::RemoteSync;
use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use pstore::db::Pool;
use pstore::models::{Data, Person, Work};
use pstore::queries::get_max_modified;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;

pub struct Jira {
    pub domain: String,
    pub user: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct JiraPerson {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct JiraWorkItemFields {
    pub summary: String,
    pub description: Option<Value>,
    pub project: Value,
    pub status: Value,
    #[serde(rename = "issuetype")]
    pub issue_type: Value,
    pub parent: Option<Value>,
    pub created: Option<DateTime<Utc>>,
    pub updated: Option<DateTime<Utc>>,
    pub creator: Option<JiraPerson>,
    pub assignee: Option<JiraPerson>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JiraWorkItem {
    pub id: String,
    #[serde(rename = "self")]
    pub url: String,
    pub key: String,
    pub fields: JiraWorkItemFields,
    #[serde(rename = "renderedFields")]
    pub rendered_fields: Option<JiraRenderedFields>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JiraRenderedFields {
    pub description: Option<String>,
}

#[derive(Deserialize, Debug)]
struct JiraSearchResponse {
    pub issues: Option<Vec<JiraWorkItem>>,
    #[serde(rename = "errorMessages")]
    pub error_messages: Option<Vec<String>>,
}

async fn process_project(
    remote_id: i64,
    client: &Client,
    domain: &str,
    user: &str,
    pat: &str,
    project: &Value,
    pool: &Pool,
) -> Result<Data> {
    let project_key = project["key"]
        .as_str()
        .ok_or_else(|| anyhow!("Project key not found"))?;
    let max_modified = get_max_modified(pool, project_key, remote_id).await?;

    let date_filter = if let Some(max_modified_date) = max_modified {
        format!(
            " AND updated > '{}'",
            max_modified_date.format("%Y-%m-%d %H:%M")
        )
    } else {
        "".to_string()
    };

    let jql = format!("project = \"{project_key}\"{date_filter} ORDER BY updated DESC");

    let mut start_at = 0;
    let max_results = 100;

    let mut set = JoinSet::new();

    loop {
        let search_body = serde_json::json!({
            "jql": jql,
            "startAt": start_at,
            "maxResults": max_results,
            "fields": [
                "summary",
                "description",
                "project",
                "status",
                "issuetype",
                "parent",
                "created",
                "updated",
                "creator",
                "assignee"
            ],
            "expand": ["renderedFields"]
        });

        let client = client.clone();
        let user = user.to_string();
        let pat = pat.to_string();
        let domain_clone = domain.to_string();

        set.spawn(async move {
            let search_url = format!("https://{domain_clone}.atlassian.net/rest/api/3/search/jql");
            let search_response = client
                .post(&search_url)
                .basic_auth(user, Some(pat))
                .json(&search_body)
                .send()
                .await?;
            let response_text = search_response.text().await?;
            let decoded_response = serde_json::from_str::<JiraSearchResponse>(&response_text)
                .map_err(|e| anyhow!("Failed to decode JiraSearchResponse: {e}. Response body: {response_text}"))?;

            if let Some(errors) = decoded_response.error_messages {
                return Err(anyhow!("Jira API returned errors: {}", errors.join(", ")));
            }

            let issues = decoded_response.issues.ok_or_else(|| anyhow!("JiraSearchResponse is missing 'issues' field and did not provide error messages."))?;

            let work: Vec<_> = issues
                .iter()
                .map(|issue| Work {
                    remote_id,
                    id: issue.id.clone(),
                    version: None,
                    url: Some(issue.url.clone()),
                    project: issue.fields.project["key"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    title: issue.fields.summary.clone(),
                    description: issue
                        .rendered_fields
                        .as_ref()
                        .and_then(|r| r.description.clone()),
                    created: issue.fields.created,
                    created_by_id: issue.fields.creator.as_ref().map(|c| c.account_id.clone()),
                    assigned_to_id: issue.fields.assignee.as_ref().map(|a| a.account_id.clone()),
                    column: None,
                    modified: issue.fields.updated,
                    state: issue.fields.status["name"].as_str().map(|s| s.to_string()),
                    work_type: issue.fields.issue_type["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    parent_id: issue
                        .fields
                        .parent
                        .as_ref()
                        .and_then(|p| p["id"].as_str())
                        .map(|s| s.to_string()),
                })
                .collect();

            let people: Vec<Person> = issues
                .iter()
                .flat_map(|issue| {
                    [
                        issue.fields.creator.as_ref(),
                        issue.fields.assignee.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(|p| Person {
                        remote_id,
                        id: p.account_id.clone(),
                        name: p.display_name.clone(),
                    })
                })
                .unique_by(|p| p.id.clone())
                .collect();

            Ok::<Data, Error>(Data { work, people })
        });

        if set.len() < max_results {
            break;
        }
        start_at += max_results;
    }

    let mut data = Data::default();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(d)) => {
                data.work.extend(d.work);
                data.people.extend(d.people);
            }
            Ok(Err(e)) => {
                eprintln!(
                    "Warning: Could not fetch work items batch for project {project_key}: {e}"
                )
            }
            Err(e) => eprintln!("Warning: Batch task failed for project {project_key}: {e}"),
        }
    }

    Ok(data)
}

#[async_trait]
impl RemoteSync for Jira {
    async fn sync(&self, client: &Client, remote_id: i64, pool: &Pool) -> Result<Data, Error> {
        let projects_url = format!("https://{}.atlassian.net/rest/api/3/project", self.domain);
        let projects_response: Vec<Value> = client
            .get(&projects_url)
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?
            .json()
            .await?;

        let mut set = JoinSet::new();

        for project in projects_response {
            let client = client.clone();
            let domain = self.domain.clone();
            let user = self.user.clone();
            let pat = self.password.clone();

            let pool = pool.clone();
            set.spawn(async move {
                process_project(remote_id, &client, &domain, &user, &pat, &project, &pool).await
            });
        }

        let mut data = Data::default();
        while let Some(res) = set.join_next().await {
            match res {
                Ok(Ok(d)) => {
                    data.work.extend(d.work);
                    data.people.extend(d.people);
                }
                Ok(Err(e)) => eprintln!("Project processing error: {e}"),
                Err(e) => eprintln!("Task join error: {e}"),
            }
        }

        Ok(data)
    }
}
