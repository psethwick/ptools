use chrono::{Datelike, NaiveDate};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

// TODO: client and task should maybe also be Option?
// or maybe I need a third variant?
#[derive(Debug, PartialEq, Eq, Serialize)]
pub enum EntryType {
    #[serde(rename = "break")]
    Break,
    #[serde(rename = "work")]
    Work {
        client: String,
        task: String,
        ticket_id: Option<String>,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub start: usize,
    pub end: Option<usize>,
    pub entry_type: EntryType,
}

impl Entry {
    pub fn duration(&self) -> Option<f64> {
        self.end.map(|end| (end as f64 - self.start as f64) / 100.0)
    }
}

#[derive(Serialize)]
pub struct Day {
    pub date: NaiveDate,
    pub entries: Vec<Entry>,
}

#[derive(Serialize)]
pub struct Task {
    desc: String,
    total: f64,
}

#[derive(Serialize)]
pub struct Group {
    client: String,
    entries: Vec<Task>,
    total: f64,
}

#[derive(Serialize)]
pub struct Report {
    groups: Vec<Group>,
    total: f64,
}

#[derive(Deserialize, Debug)]
struct Author {
    #[serde(rename = "displayName")]
    name: String,
    #[serde(rename = "accountId")]
    account_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Worklog {
    author: Author,
    id: String,
    started: String,
}

#[derive(Deserialize, Debug)]
struct Worklogs {
    worklogs: Vec<Worklog>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewWorklog {
    time_spent_seconds: u64,
}

pub struct JiraDetails {
    pub username: String,
    pub url: String,
    pub password: String,
}

impl Day {
    pub fn total_work(&self, client_filter: Option<&str>) -> f64 {
        self.entries
            .iter()
            .filter(|e| {
                matches!(&e.entry_type,
                EntryType::Work { client, .. }
                    if client_filter.is_none() || client_filter.unwrap() == client)
            })
            .map(|e| e.duration().unwrap_or(0.0))
            .sum()
    }

    pub async fn sync(&self, jira_details: &JiraDetails) -> Result<(), reqwest::Error> {
        let client = reqwest::Client::new();

        // Fetch current user's accountId for accurate worklog filtering
        let myself_url = format!("{}/rest/api/2/myself", &jira_details.url);
        let myself_response = client
            .get(&myself_url)
            .basic_auth(&jira_details.username, Some(&jira_details.password))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        let current_user_account_id = myself_response["accountId"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                reqwest::Error::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to get accountId from /myself endpoint",
                ))
            })?;

        for (ticket_id, duration) in self
            .entries
            .iter()
            .filter(|e| e.end.is_some())
            .filter_map(|e| match &e.entry_type {
                EntryType::Work { ticket_id, .. } => {
                    ticket_id.clone().and_then(|t| e.duration().map(|d| (t, d)))
                }
                _ => None,
            })
            .into_group_map()
        {
            let total_seconds = (duration.iter().sum::<f64>() * 3600.0) as u64;

            let worklogs_url =
                format!("{}/rest/api/2/issue/{ticket_id}/worklog", &jira_details.url);

            let worklogs = client
                .get(&worklogs_url)
                .basic_auth(&jira_details.username, Some(&jira_details.password))
                .send()
                .await?
                .json::<Worklogs>()
                .await?;
            dbg!(&worklogs);

            let existing_worklog = worklogs.worklogs.iter().find(|w| {
                // Compare account_id to accurately identify worklogs by the current user
                if w.author.account_id.as_ref() != Some(&current_user_account_id) {
                    return false;
                }
                if let Ok(started_date) = NaiveDate::parse_from_str(&w.started[0..10], "%Y-%m-%d") {
                    return started_date.year() == self.date.year()
                        && started_date.month() == self.date.month()
                        && started_date.day() == self.date.day();
                }
                false
            });

            let worklog_body = NewWorklog {
                time_spent_seconds: total_seconds,
            };

            if let Some(existing) = existing_worklog {
                let update_url = format!("{worklogs_url}/{}", existing.id);
                println!("Updating worklog for {ticket_id}: {total_seconds}s");
                // client
                //     .put(update_url)
                //     .basic_auth(username, Some(password))
                //     .json(&worklog_body)
                //     .send()
                //     .await?;
            } else {
                println!("Creating new worklog for {ticket_id}: {total_seconds}s");
                // client
                //     .post(worklogs_url)
                //     .basic_auth(username, Some(password))
                //     .json(&worklog_body)
                //     .send()
                //     .await?;
            }
        }
        Ok(())
    }

    pub fn report_str(&self, client_filter: Option<&str>) -> String {
        let mut result = String::with_capacity(100);

        let total = self.total_work(client_filter);
        if total == 0.0 {
            return result;
        }
        result.push_str(&format!(
            "{}: {}
",
            self.date.format("%A, %d %B"),
            total
        ));

        for (client, task_duration) in self
            .entries
            .iter()
            .filter(|e| e.end.is_some())
            .filter_map(|e| match &e.entry_type {
                EntryType::Work {
                    client,
                    task,
                    ticket_id: _,
                } => Some((
                    client.as_str(),
                    (task.as_str(), e.duration().unwrap_or(0.0)),
                )),
                _ => None,
            })
            .filter(|(client, _)| client_filter.is_none() || client_filter.as_ref() == Some(client))
            .into_group_map()
        {
            let client_total: f64 = task_duration.iter().map(|(_, duration)| duration).sum();
            result.push_str(&format!("  {client}:  {client_total}\n"));
            for (task, durations) in task_duration.iter().cloned().into_group_map() {
                if !task.is_empty() {
                    result.push_str(&format!(
                        "    {}:  {}\n",
                        task,
                        durations.iter().cloned().sum::<f64>()
                    ));
                }
            }
        }
        result.push('\n');
        result
    }
}
