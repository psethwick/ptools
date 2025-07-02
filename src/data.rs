use chrono::{DateTime, Utc};

use crate::config::SourceConfig;

pub struct Work {
    source: SourceConfig,
    project: String,
    id: String,
    parent_id: Option<String>,
    title: String,
    work_type: String,
    description: Option<String>,
    version: Option<String>,
    state: Option<String>,
    created_id: Option<String>,
    assigned_id: Option<String>,
    column: Option<String>,
    created: Option<DateTime<Utc>>,
    modified: Option<DateTime<Utc>>,
    url: Option<String>,
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
