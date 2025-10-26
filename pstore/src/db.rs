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

pub async fn init() -> Result<SqlitePool> {
    let data_dir = get_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("pstore.db");
    let pool =
        SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap())).await?;
    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(&pool)
        .await?;
    migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
