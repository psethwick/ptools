use crate::config::SourceConfig;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Work {
    pub source: SourceConfig,
    pub project: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub work_type: String,
    pub description: Option<String>,
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
    source: SourceConfig,
    id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // most of these will probably be Work
pub enum Data {
    Work(Work),
    Person(Person),
    // Pull Requests?
    // Person?
    // Event?
}
