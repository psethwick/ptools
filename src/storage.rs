use crate::data::Data;
use anyhow::{Ok, Result, anyhow};
use serde_json::to_writer_pretty;
use std::{fs::File, io::BufReader, path::PathBuf};

fn get_data_path(name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(name);
        path
    })
}

pub fn save(data: &Vec<Data>, name: &str) -> Result<()> {
    if let Some(path) = get_data_path(name) {
        if let Some(parent_dir) = path.parent() {
            std::fs::create_dir_all(parent_dir)?;
        }

        let file = File::create(&path)?;
        to_writer_pretty(file, &data)?;
        println!("Successfully serialized items to {path:?}");
    }
    Ok(())
}

pub fn load(name: &str) -> Result<Vec<Data>> {
    match get_data_path(name) {
        Some(path) => {
            let file = File::open(path)?;
            let reader = BufReader::new(file);

            Ok(serde_json::from_reader(reader)?)
        }
        None => Result::Err(anyhow!("can't read {name}")),
    }
}
