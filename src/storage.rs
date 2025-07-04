use crate::data::SourceData;
use anyhow::{Ok, Result};
use serde_json::to_writer_pretty;
use std::{fs::File, path::PathBuf};

fn get_data_path(folder: &str, name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(folder);
        path.push(name);
        path
    })
}

pub fn save(data: &Vec<SourceData>) -> Result<()> {
    for sd in data {
        if let Some(path) = get_data_path("work", &format!("{}.json", sd.source.get_filename())) {
            if let Some(parent_dir) = path.parent() {
                std::fs::create_dir_all(parent_dir)?;
            }

            let file = File::create(&path)?;
            to_writer_pretty(file, &sd.work)?;
            println!("Successfully serialized items to {path:?}");
        }
        // TODO: serialize people, etc to other folders
        // we want lake-style data, schema per folder
    }
    Ok(())
}

pub fn load() -> Result<Vec<SourceData>> {
    todo!()
    // match get_data_path(name) {
    //     Some(path) => {
    //         let file = File::open(path)?;
    //         let reader = BufReader::new(file);
    //
    //         Ok(serde_json::from_reader(reader)?)
    //     }
    //     None => Result::Err(anyhow!("can't read {name}")),
    // }
}
