use crate::config::SourceConfig;
use crate::source::SERVICE_NAME;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::path::PathBuf;

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

#[derive(Serialize, Deserialize)]
pub struct Person {
    id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
pub struct SourceData {
    pub source: SourceConfig,
    pub work: Vec<Work>,
    pub people: Vec<Person>,
}
// TODO: Pull Requests?
// Event?

pub fn get_data_path(folder: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push(SERVICE_NAME);
        path.push(folder);
        path.push(name);
        path
    })
}
