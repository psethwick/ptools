use anyhow::Result;
use chrono::NaiveDate;
use itertools::Itertools;
use pstore::models::{Timesheet, decimal_hours_to_jira};
use serde::Serialize;

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

    pub async fn save_to_pstore(&self, remote_name: &str) -> Result<()> {
        let pool = pstore::db::init().await?;
        let remotes = pstore::queries::get_remotes(&pool).await?;

        let mut tx = pool.begin().await?;

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
            if let Some(remote) = remotes
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(remote_name))
            {
                let total_hours = duration.iter().sum::<f64>();
                let date_str = self.date.format("%Y-%m-%d").to_string();
                let duration_str = decimal_hours_to_jira(total_hours);

                let ts = Timesheet {
                    remote_id: remote.id,
                    ticket_id: ticket_id.clone(),
                    date: date_str,
                    duration: duration_str.clone(),
                };
                ts.save(&mut *tx).await?;
            } else {
                eprintln!("Warning: Could not find remote for ticket {ticket_id}");
            }
        }

        tx.commit().await?;
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
