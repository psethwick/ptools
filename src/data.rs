use crate::config::Config;
use crate::config::SourceConfig;
use crate::source::Source;
use anyhow::{Ok, Result, anyhow};
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

impl SourceData {
    fn get_data_path(folder: &str, name: &str) -> Option<PathBuf> {
        dirs::data_dir().map(|mut path| {
            path.push("yoink");
            path.push(folder);
            path.push(name);
            path
        })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(path) =
            Self::get_data_path("work", &format!("{}.json", self.source.get_filename()))
        {
            if let Some(parent_dir) = path.parent() {
                std::fs::create_dir_all(parent_dir)?;
            }

            let file = File::create(&path)?;
            to_writer_pretty(file, &self.work)?;
            println!("Successfully serialized items to {path:?}");
        }
        // TODO: serialize people, etc to other folders
        // we want lake-style data, schema per folder
        Ok(())
    }

    pub fn load() -> Result<Vec<SourceData>> {
        let config = Config::load()?;

        config
            .sources()
            .into_iter()
            .map(|s| {
                let filename = format!("{}.json", s.source_config().get_filename());
                match Self::get_data_path("work", &filename) {
                    Some(path) => {
                        let file = File::open(path)?;
                        let reader = BufReader::new(file);

                        Ok(serde_json::from_reader(reader)?)
                    }
                    None => Result::Err(anyhow!("can't read {filename}")),
                }
            })
            .collect()
    }
}
