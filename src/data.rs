use chrono::{DateTime, Utc};

use crate::config::SourceConfig;

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

pub struct Person {
    source: SourceConfig,
    id: String,
    name: String,
}

pub enum Data {
    Work(Work),
    Person(Person),
    // Pull Requests?
    // Person?
    // Event?
}
