use anyhow::Result;
use chrono::NaiveDate;
use itertools::Itertools;
use pstore::models::{Timesheet, decimal_hours_to_jira};
use pstore::db::Pool;
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

    /// Save timesheet entries to pstore. Clears any existing entries for this
    /// remote and date first, then inserts the new entries.
    pub async fn save_to_pstore(&self, remote_name: &str) -> Result<()> {
        let pool = pstore::db::init().await?;
        self.do_save_to_pstore(&pool, remote_name).await
    }

    #[cfg(feature = "test-utils")]
    /// Save timesheet entries to a specific pool. Useful for testing.
    pub async fn save_to_pstore_with_pool(&self, pool: &Pool, remote_name: &str) -> Result<()> {
        self.do_save_to_pstore(pool, remote_name).await
    }

    async fn do_save_to_pstore(&self, pool: &Pool, remote_name: &str) -> Result<()> {
        let remotes = pstore::queries::get_remotes(pool).await?;

        // Find the remote matching the given name
        let remote = match remotes
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(remote_name))
        {
            Some(r) => r,
            None => {
                eprintln!("Warning: Could not find remote '{remote_name}'");
                return Ok(());
            }
        };

        let date_str = self.date.format("%Y-%m-%d").to_string();

        // Clear existing entries for this remote and date to ensure clean state
        // This handles the case where the user fixed a typo in ticket ID
        pstore::queries::clear_timesheet_entries(pool, remote.id, &date_str).await?;

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
            let total_hours = duration.iter().sum::<f64>();
            let duration_str = decimal_hours_to_jira(total_hours);

            let ts = Timesheet {
                remote_id: remote.id,
                ticket_id: ticket_id.clone(),
                date: date_str.clone(),
                duration: duration_str,
            };
            ts.save(&mut *tx).await?;
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