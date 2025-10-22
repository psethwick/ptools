use crate::{SERVICE_NAME, azure_devops::AzureDevops, jira::Jira};
use anyhow::{Error, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{Database, Decode, Encode, Executor, FromRow, Sqlite, SqlitePool, Type};
use std::fmt::Display;

#[async_trait]
pub trait RemoteSync: Send + Sync {
    async fn sync(&self, client: &Client, pool: &SqlitePool, source_id: i64) -> Result<()>;
}

#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum Kind {
    AzureDevops = 0,
    Jira = 1,
}

impl Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let printable = match self {
            Kind::AzureDevops => "azure_devops",
            Kind::Jira => "jira",
        };
        write!(f, "{printable}")
    }
}

#[derive(PartialEq, Eq, Clone, FromRow, Debug)]
pub struct Source {
    pub id: i64,
    #[sqlx(try_from = "i32")]
    pub kind: Kind,
    pub name: String,
}

impl TryFrom<i32> for Kind {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Kind::AzureDevops),
            1 => Ok(Kind::Jira),
            _ => Err(format!("Invalid kind value: {value}")),
        }
    }
}

impl From<Kind> for i32 {
    fn from(kind: Kind) -> Self {
        kind as i32
    }
}

impl<DB: Database> Type<DB> for Kind
where
    i32: Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <i32 as Type<DB>>::type_info()
    }
}

impl<'r, DB: Database> Decode<'r, DB> for Kind
where
    i32: Decode<'r, DB>,
{
    fn decode(value: <DB as Database>::ValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let n = <i32 as Decode<DB>>::decode(value)?;
        Kind::try_from(n).map_err(Into::into)
    }
}

impl<'q, DB: Database> Encode<'q, DB> for Kind
where
    i32: Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let value: i32 = (*self).into();
        <i32 as Encode<DB>>::encode_by_ref(&value, buf)
    }
}

impl Source {
    pub async fn sync(&self, client: &Client, pool: &SqlitePool) -> Result<(), Error> {
        match self.kind {
            Kind::AzureDevops => {
                self.get_password()
                    .map(|pat| AzureDevops {
                        org: self.name.to_owned(),
                        pat: pat.to_owned(),
                    })?
                    .sync(client, pool, self.id)
                    .await?;
            }
            Kind::Jira => {
                let password_json = self.get_password()?;
                let password_data: serde_json::Value = serde_json::from_str(&password_json)?;
                let user = password_data["user"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Jira user not found in password data"))?
                    .to_owned();
                let pat = password_data["password"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Jira password not found in password data"))?
                    .to_owned();
                Jira {
                    domain: self.name.to_owned(),
                    user,
                    password: pat,
                }
                .sync(client, pool, self.id)
                .await?;
            }
        }
        Ok(())
    }

    pub async fn add(pool: &SqlitePool, kind: Kind, name: &str, password: String) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO source (kind, name) VALUES (?, ?)")
            .bind(kind)
            .bind(name)
            .execute(pool)
            .await?;

        let entry = Entry::new(SERVICE_NAME, &format!("{kind}-{name}"))?;
        entry.set_password(&password)?;
        Ok(())
    }

    fn get_password(&self) -> Result<String, Error> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind, self.name))?;
        Ok(entry.get_password()?)
    }

    pub fn delete_password(&self) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind, self.name))?;
        Ok(entry.delete_credential()?)
    }
}

#[derive(FromRow, Debug, Serialize, Deserialize)]
pub struct Work {
    pub source_id: i64,
    pub project: String,
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
    pub work_type: String,
    pub version: Option<String>,
    pub state: Option<String>,
    pub created_by_id: Option<String>,
    pub assigned_to_id: Option<String>,
    pub column: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub modified: Option<DateTime<Utc>>,
    pub url: Option<String>,
}

impl Work {
    pub async fn save<'a, E>(&self, executor: E) -> Result<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            "INSERT OR REPLACE INTO work (source_id, id, project, title, parent_id, description, work_type, version, state, created_by_id, assigned_to_id, column, created, modified, url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.source_id)
        .bind(&self.id)
        .bind(&self.project)
        .bind(&self.title)
        .bind(&self.parent_id)
        .bind(&self.description)
        .bind(&self.work_type)
        .bind(&self.version)
        .bind(&self.state)
        .bind(&self.created_by_id)
        .bind(&self.assigned_to_id)
        .bind(&self.column)
        .bind(self.created)
        .bind(self.modified)
        .bind(&self.url)
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Person {
    pub source_id: i64,
    pub id: String,
    pub name: String,
}

impl Person {
    pub async fn save<'a, E>(&self, executor: E) -> Result<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            "INSERT OR REPLACE INTO person (source_id, id, name)
             VALUES (?, ?, ?)",
        )
        .bind(self.source_id)
        .bind(&self.id)
        .bind(&self.name)
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Data {
    pub work: Vec<Work>,
    pub people: Vec<Person>,
}

// TODO: Pull Requests?
// Event?

pub enum Sources {
    AzureDevops(AzureDevops),
    Jira(Jira),
}

pub async fn get_sources(pool: &SqlitePool) -> Result<Vec<Source>> {
    let s = sqlx::query_as::<_, Source>("SELECT id, kind, name FROM source")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    Ok(s)
}

pub async fn remove_source(pool: &SqlitePool, sc_to_remove: &Source) -> Result<()> {
    sc_to_remove.delete_password()?;
    sqlx::query("DELETE FROM source WHERE id=?")
        .bind(sc_to_remove.id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_max_modified(
    pool: &SqlitePool,
    project: &str,
    source_id: i64,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let max_modified = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        r#"SELECT MAX(modified) FROM work WHERE source_id = ? and project = ?"#,
    )
    .bind(source_id)
    .bind(project)
    .fetch_one(pool)
    .await?;

    Ok(max_modified)
}
