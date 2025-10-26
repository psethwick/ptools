pub mod azure_devops;
pub mod jira;
pub mod remote;

use anyhow::{anyhow, Error, Result};
use pstore::models::{Data, Kind, Remote};
use pstore::{db::Pool, queries::get_password};
use reqwest::Client;

use crate::azure_devops::AzureDevops;
use crate::jira::Jira;
use crate::remote::RemoteSync;

pub async fn sync_remote(remote: &Remote, client: &Client, pool: &Pool) -> Result<(), Error> {
    let data: Data = match remote.kind {
        Kind::AzureDevops => {
            get_password(remote)
                .map(|pat| AzureDevops {
                    org: remote.name.to_owned(),
                    pat: pat.to_owned(),
                })?
                .sync(client, remote.id)
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
            .sync(client, remote.id)
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
