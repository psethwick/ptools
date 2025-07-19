use anyhow::{Ok, Result};
use chrono::{DateTime, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Sqlite, SqlitePool};

use crate::{SERVICE_NAME, azure_devops::AzureDevops, source::Source};

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, FromRow, Debug)]
pub struct SourceConfig {
    pub id: i64,
    pub kind: String,
    pub name: String,
}

impl SourceConfig {
    pub fn get_source(&self) -> Result<impl Source + use<>> {
        match self.kind.as_str() {
            "azure_devops" => self.get_password().map(|pat| AzureDevops {
                org: self.name.to_owned(),
                pat: pat.to_owned(),
                source_id: self.id,
            }),
            &_ => todo!(),
        }
    }

    pub fn get_filename(&self) -> String {
        format!("{}-{}", self.kind, self.name)
    }

    fn get_password(&self) -> Result<String, anyhow::Error> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind, self.name))?;
        Ok(entry.get_password()?)
    }

    pub fn store_password(&self, password: &str) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", self.kind, self.name))?;
        entry.set_password(password)?;
        Ok(())
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

#[derive(Serialize, Deserialize, PartialEq, Eq)]
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
}

pub async fn get_sources(pool: &SqlitePool) -> Result<Vec<impl Source + use<>>> {
    sqlx::query_as::<_, SourceConfig>("SELECT id, kind, name, secrets FROM source")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|s| s.get_source())
        .collect()
}

pub async fn remove_source(pool: &SqlitePool, sc_to_remove: &SourceConfig) -> Result<()> {
    sc_to_remove.delete_password()?;
    sqlx::query("DELETE FROM source WHERE id=?")
        .bind(sc_to_remove.id)
        .execute(pool)
        .await?;
    Ok(())
}
