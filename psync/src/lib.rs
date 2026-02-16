pub mod azure_devops;
pub mod jira;
pub mod remote;

use anyhow::{Error, Result, anyhow};
use itertools::Itertools;
use pstore::models::{Data, Kind, Remote};
use pstore::{db::Pool, queries::{get_password, get_unsynced_timesheets, mark_timesheets_synced}};
use reqwest::Client;
use tokio::task::JoinSet;

use crate::azure_devops::AzureDevops;
use crate::jira::Jira;
use crate::remote::RemoteSync;

pub async fn pull_remote_work(remote: &Remote, client: &Client, pool: &Pool) -> Result<(), Error> {
    let data: Data = match remote.kind {
        Kind::AzureDevops => {
            get_password(remote)
                .map(|pat| AzureDevops {
                    org: remote.name.to_owned(),
                    pat: pat.to_owned(),
                })?
                .sync(client, remote.id, pool)
                .await?
        }
        Kind::Jira => {
            let password_json = get_password(remote)?;
            let password_data: serde_json::Value = serde_json::from_str(&password_json)?;
            let user = password_data["user"]
                .as_str()
                .ok_or_else(|| anyhow!("Jira user not found in password data"))?
                .to_owned();
            let pat = password_data["password"]
                .as_str()
                .ok_or_else(|| anyhow!("Jira password not found in password data"))?
                .to_owned();
            Jira {
                domain: remote.name.to_owned(),
                user,
                password: pat,
            }
            .sync(client, remote.id, pool)
            .await?
        }
    };

    let mut tx = pool.begin().await?;
    for work_item in data.work {
        work_item.save(&mut *tx).await?;
    }
    for person in data.people {
        person.save(&mut *tx).await?;
    }
    tx.commit().await?;

    Ok(())
}

pub async fn push_time(remote: &Remote, client: &Client, pool: &Pool) -> Result<(), Error> {
    if remote.kind != Kind::Jira {
        return Ok(());
    }

    let password_json = get_password(remote)?;
    let password_data: serde_json::Value = serde_json::from_str(&password_json)?;
    let user = password_data["user"]
        .as_str()
        .ok_or_else(|| anyhow!("Jira user not found in password data"))?
        .to_owned();
    let pat = password_data["password"]
        .as_str()
        .ok_or_else(|| anyhow!("Jira password not found in password data"))?
        .to_owned();

    let jira = Jira {
        domain: remote.name.to_owned(),
        user,
        password: pat,
    };

    let rows = get_unsynced_timesheets(pool, remote.id).await?;
    if rows.is_empty() {
        println!("No unsynced timesheet entries for {}", remote.name);
        return Ok(());
    }

    let account_id = jira.get_myself(client).await?;

    // Group by (ticket_id, date) and sum durations (parse Xh Ym back to seconds)
    let grouped: Vec<_> = rows
        .into_iter()
        .into_group_map_by(|r| (r.ticket_id.clone(), r.date.clone()))
        .into_iter()
        .collect();

    let mut set = JoinSet::new();

    for ((ticket_id, date), group_rows) in grouped {
        let client = client.clone();
        let jira_domain = jira.domain.clone();
        let jira_user = jira.user.clone();
        let jira_password = jira.password.clone();
        let account_id = account_id.clone();
        let row_ids: Vec<i64> = group_rows.iter().map(|r| r.id).collect();
        // The duration is already in Jira format (e.g. "2h 30m"), just use the first row's
        // since each (ticket, date) combo should have one row after the upsert
        let duration = group_rows[0].duration.clone();

        set.spawn(async move {
            let jira = Jira {
                domain: jira_domain,
                user: jira_user,
                password: jira_password,
            };

            let worklogs = jira.get_worklogs(&client, &ticket_id).await?;

            // Find existing worklog by same author and same date
            let existing = worklogs.iter().find(|wl| {
                wl.author.account_id == account_id && wl.started.starts_with(&date)
            });

            match existing {
                Some(wl) => {
                    // Parse our duration to seconds for comparison
                    let our_seconds = parse_jira_duration_to_seconds(&duration);
                    if wl.time_spent_seconds == our_seconds {
                        println!("{ticket_id} ({date}): already up to date ({duration})");
                    } else {
                        println!("{ticket_id} ({date}): updating to {duration}");
                        jira.update_worklog(&client, &ticket_id, &wl.id, &duration).await?;
                    }
                }
                None => {
                    println!("{ticket_id} ({date}): logging {duration}");
                    jira.add_worklog(&client, &ticket_id, &date, &duration).await?;
                }
            }

            Ok::<Vec<i64>, Error>(row_ids)
        });
    }

    let mut all_synced_ids = Vec::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(ids)) => all_synced_ids.extend(ids),
            Ok(Err(e)) => eprintln!("Worklog sync error: {e}"),
            Err(e) => eprintln!("Task join error: {e}"),
        }
    }

    if !all_synced_ids.is_empty() {
        mark_timesheets_synced(pool, &all_synced_ids).await?;
        println!("Marked {} row(s) as synced", all_synced_ids.len());
    }

    Ok(())
}

fn parse_jira_duration_to_seconds(duration: &str) -> i64 {
    let mut seconds = 0i64;
    for part in duration.split_whitespace() {
        if let Some(h) = part.strip_suffix('h') {
            if let Ok(n) = h.parse::<i64>() {
                seconds += n * 3600;
            }
        } else if let Some(m) = part.strip_suffix('m') {
            if let Ok(n) = m.parse::<i64>() {
                seconds += n * 60;
            }
        }
    }
    seconds
}
