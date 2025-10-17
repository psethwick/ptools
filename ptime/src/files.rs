use crate::entries::Day;
use crate::parse::parse_time;
use chrono::naive::NaiveDate;
use chrono::Local;
use std::{
    env, fs,
    io::{Error, Write},
    path::{Path, PathBuf},
};

fn get_day_path(day: NaiveDate) -> PathBuf {
    let base = env::var("TIMESHEET_BASE_FOLDER").expect("Must set base folder to store timesheets");
    let path = Path::new(&base);
    let filename = day.format("%Y-%m-%d.log").to_string();
    path.join(filename)
}

pub fn get_today_path() -> PathBuf {
    get_day_path(Local::now().date_naive())
}

fn add_entry(entry: &str, day: NaiveDate) -> Result<(), Error> {
    let (_, new_time) = parse_time(entry).expect("entry time is incorrect");
    if let Some(existing) = Day::new(day) {
        let max = existing.entries.iter().map(|e| e.start).max().unwrap();
        if new_time < max {
            return Err(Error::other("new time is before last time"));
        }
    }

    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(get_day_path(day))?;

    file.sync_all()?;
    writeln!(file, "{entry}")
}

pub fn add_today_entry(entry: &str) -> Result<(), Error> {
    add_entry(entry, Local::now().date_naive())
}

pub fn get_day_contents(day: NaiveDate) -> Option<String> {
    fs::read_to_string(get_day_path(day)).ok()
}

#[cfg(test)]
mod tests {
    use crate::files::*;
    use temp_env::with_var;
    use tempfile::tempdir;
    fn with_ts_base(closure: fn(p: PathBuf) -> ()) -> Result<(), Error> {
        let temp_dir = tempdir()?;
        let path = temp_dir.into_path();
        let path_str = format!("{}", path.to_str().unwrap());
        with_var("TIMESHEET_BASE_FOLDER", Some(path_str), || {
            closure(path.clone())
        });
        Ok(())
    }

    #[test]
    fn day_path() {
        with_ts_base(|path| {
            assert_eq!(
                get_day_path(NaiveDate::from_ymd_opt(2021, 4, 25).unwrap()),
                path.join("2021-04-25.log")
            )
        })
        .unwrap();
    }

    #[test]
    fn add_entry_empty() {
        with_ts_base(|_path| {
            add_today_entry("1234 testing testing").unwrap();
            assert_eq!(
                get_day_contents(Local::now().date_naive()),
                Some("1234 testing testing\n".to_string())
            )
        })
        .unwrap();
    }

    #[test]
    fn add_entry_existing() {
        with_ts_base(|_path| {
            let today = Local::now().date_naive();
            add_today_entry("1000 testing testing").unwrap();
            add_today_entry("1100 yes no").unwrap();
            assert_eq!(
                get_day_contents(today),
                Some("1000 testing testing\n1100 yes no\n".to_string())
            );
        })
        .unwrap();
    }

    #[test]
    fn add_entry_with_time_travel() {
        with_ts_base(|_path| {
            add_today_entry("2345 testing testing").unwrap();
            let result = add_today_entry("1234 yes no");
            println!("{result:?}");
            assert!(matches!(result, Err(_)));
        })
        .unwrap();
    }
}
