use crate::models::{Source, Work, Person, Kind, Timesheet};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Executor, Sqlite, SqlitePool};
use keyring::Entry;
use crate::SERVICE_NAME;

pub async fn get_sources(pool: &SqlitePool) -> Result<Vec<Source>> {
    let s = sqlx::query_as::<_, Source>("SELECT id, kind, name FROM source")
        .fetch_all(pool)
        .await?;
    Ok(s)
}

pub async fn remove_source(pool: &SqlitePool, sc_to_remove: &Source) -> Result<()> {
    delete_password(sc_to_remove)?;
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

pub async fn add_source(pool: &SqlitePool, kind: Kind, name: &str, password: String) -> Result<()> {
    sqlx::query("INSERT OR REPLACE INTO source (kind, name) VALUES (?, ?)")
        .bind(kind)
        .bind(name)
        .execute(pool)
        .await?;

    let entry = Entry::new(SERVICE_NAME, &format!("{kind}-{name}"))?;
    entry.set_password(&password)?;
    Ok(())
}

pub fn get_password(source: &Source) -> Result<String> {
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", source.kind, source.name))?;
    Ok(entry.get_password()?)
}

pub fn delete_password(source: &Source) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, &format!("{}-{}", source.kind, source.name))?;
    Ok(entry.delete_credential()?)
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
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub async fn get_work(pool: &SqlitePool) -> Result<Vec<Work>> {
    let work_items = sqlx::query_as::<_, Work>("SELECT * FROM work")
        .fetch_all(pool)
        .await?;
    Ok(work_items)
}

impl Timesheet {
    pub async fn save<'a, E>(&self, executor: E) -> Result<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            r#"
            INSERT INTO timesheet (source_id, ticket_id, date, duration_seconds)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(source_id, ticket_id, date) DO UPDATE SET
                duration_seconds = excluded.duration_seconds,
                synced = 0
            WHERE timesheet.duration_seconds != excluded.duration_seconds
            "#,
        )
        .bind(self.source_id)
        .bind(&self.ticket_id)
        .bind(&self.date)
        .bind(self.duration_seconds)
        .execute(executor)
        .await?;

        Ok(())
    }
}

