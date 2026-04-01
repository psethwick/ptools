use crate::language::Verb;
use crate::picker::{Picker, ToMeatspace, VerbHandler};
use anyhow::{anyhow, Result};
use chrono::naive::NaiveDate;
use chrono::Local;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Notes {}

fn expand_tilde<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
    let p = path.as_ref();
    if !p.starts_with("~") {
        return Some(p.to_path_buf());
    }
    if p == Path::new("~") {
        return dirs::home_dir();
    }
    dirs::home_dir().map(|mut h| {
        if h == Path::new("/") {
            // Corner case: `h` root directory;
            // don't prepend extra `/`, just drop the tilde.
            match p.strip_prefix("~") {
                Ok(p) => p.to_path_buf(),
                Err(_) => PathBuf::new(),
            }
        } else {
            match p.strip_prefix("~/") {
                Ok(p) => {
                    h.push(p);
                    h
                }
                Err(_) => PathBuf::new(),
            }
        }
    })
}

impl Picker for Notes {
    fn name(&self) -> &str {
        "notes"
    }

    fn verbs(&self) -> Option<Vec<(Verb, VerbHandler)>> {
        Some(vec![(
            Verb::Sink("daily".to_owned()),
            Box::new(append_to_daily_note),
        )])
    }
}

fn append_to_daily_note(input: &str) -> Result<Option<ToMeatspace>> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(get_day_path(Local::now().date_naive())?)?;

    file.sync_all()?;
    writeln!(file, "{input}")?;
    Ok(ToMeatspace::Close.into())
}

fn get_day_path(day: NaiveDate) -> Result<PathBuf> {
    let base = "~/notes/daily/";
    if let Some(path) = expand_tilde(Path::new(&base)) {
        let filename = day.format("%Y-%m-%d.md").to_string();
        Ok(path.join(filename))
    } else {
        Err(anyhow!("problem expanding notes directory"))
    }
}
