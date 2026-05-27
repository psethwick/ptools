use anyhow::{anyhow, Result};
use sqlx::{migrate, SqlitePool};
use std::path::PathBuf;

use crate::SERVICE_NAME;

pub type Pool = SqlitePool;

fn get_data_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|mut path| {
            path.push(SERVICE_NAME);
            path
        })
        .ok_or(anyhow!("Couldn't determine data directory"))
}

fn get_db_path() -> Result<PathBuf> {
    if let Ok(custom_path) = std::env::var("PSTORE_DB_PATH") {
        let path = PathBuf::from(&custom_path);
        // If it looks like a file path (has extension or ends with .db), use as-is
        // Otherwise treat as directory and append pstore.db
        if path.extension().is_some() || custom_path.ends_with(".db") {
            Ok(path)
        } else {
            Ok(path.join("pstore.db"))
        }
    } else {
        let mut path = get_data_dir()?;
        path.push("pstore.db");
        Ok(path)
    }
}

pub async fn init() -> Result<SqlitePool> {
    let db_path = get_db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool =
        SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(&pool)
        .await?;
    migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(feature = "test-utils")]
/// Initialize database with a specific path (for testing).
/// Creates the necessary tables directly without using migrations.
pub async fn init_with_path(db_path: &PathBuf) -> Result<SqlitePool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool =
        SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(&pool)
        .await?;

    // Create tables directly for testing
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS remote (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind INTEGER NOT NULL,
            name TEXT NOT NULL UNIQUE
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS timesheet (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            remote_id INTEGER NOT NULL,
            ticket_id TEXT NOT NULL,
            date TEXT NOT NULL,
            duration TEXT NOT NULL,
            synced INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (remote_id) REFERENCES remote (id),
            UNIQUE(remote_id, ticket_id, date)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}
