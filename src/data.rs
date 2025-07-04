use crate::config::SourceConfig;
use crate::source::{self, SERVICE_NAME};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::to_writer_pretty;
use std::io::BufReader;
use std::{fs::File, path::PathBuf};

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

pub fn get_data_path(folder: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push(SERVICE_NAME);
        path.push(folder);
        path.push(name);
        path
    })
}

impl SourceData {
    pub fn load(source: SourceConfig) -> Result<Self> {
        let filename = source.get_filename();
        let work_path = get_data_path("work", &format!("{}.json", &filename))
            .ok_or(anyhow!(format!("{filename} couldn't be opened")))?;

        let file = File::open(work_path)?;
        let reader = BufReader::new(file);
        let work = serde_json::from_reader(reader)?;

        Ok(SourceData {
            // TODO: should sourceconfig own this?
            // let's keep it here at least until we get to parquet
            source,
            work,
            people: vec![],
        })
    }

    pub fn save(&self) -> Result<()> {
        let filename = self.source.get_filename();
        let path = get_data_path("work", &format!("{}.json", self.source.get_filename()))
            .ok_or(anyhow!(format!("{filename} couldn't be opened")))?;
        if let Some(parent_dir) = path.parent() {
            std::fs::create_dir_all(parent_dir)?;
        }

        let file = File::create(&path)?;
        to_writer_pretty(file, &self.work)?;
        println!("Successfully serialized items to {path:?}");
        // TODO: serialize people, etc to other folders
        // we want lake-style data, schema per folder
        Ok(())
    }
}
