use crate::config::SourceConfig;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
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
    // Pull Requests?
    // Event?
}
