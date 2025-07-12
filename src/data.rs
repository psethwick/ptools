use anyhow::{Ok, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Sqlite};

#[derive(FromRow, Debug, Serialize, Deserialize)]
pub struct Work {
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
    pub async fn save<'a, E>(&self, executor: E, source: &str) -> Result<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            "INSERT OR REPLACE INTO work (source, id, project, title, parent_id, description, work_type, version, state, created_by_id, assigned_to_id, column, created, modified, url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(source)
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

#[derive(Serialize, Deserialize)]
pub struct Person {
    id: String,
    name: String,
}

impl Person {
    pub async fn save<'a, E>(&self, executor: E, source: &str) -> Result<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            "INSERT OR REPLACE INTO person (source, id, name)
             VALUES (?, ?, ?)",
        )
        .bind(source)
        .bind(&self.id)
        .bind(&self.name)
        .execute(executor)
        .await?;

        Ok(())
    }
}

// TODO: Pull Requests?
// Event?
