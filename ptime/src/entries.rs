use chrono::NaiveDate;
use itertools::Itertools;
use serde::Serialize;

    // TODO: client and task should maybe also be Option?
    // or maybe I need a third variant?
#[derive(Debug, PartialEq, Eq, Serialize)]
pub enum EntryType {
    #[serde(rename = "break")]
    Break,
    #[serde(rename = "work")]
    Work { client: String, task: String, ticket_id: Option<String> },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub start: usize,
    pub end: Option<usize>,
    pub entry_type: EntryType,
}

impl Entry {
    pub fn duration(&self) -> Option<f64> {
        self.end.map(|end| (end as f64 - self.start as f64) / 100.0)
    }
}

#[derive(Serialize)]
pub struct Day {
    pub date: NaiveDate,
    pub entries: Vec<Entry>,
}

#[derive(Serialize)]
pub struct Task {
    desc: String,
    total: f64
}

#[derive(Serialize)]
pub struct Group {
    client: String,
    entries: Vec<Task>,
    total: f64,
}

#[derive(Serialize)]
pub struct Report {
    groups: Vec<Group>,
    total: f64,
}

impl Day {
    pub fn total_work(&self, client_filter: Option<&str>) -> f64 {
        self.entries
            .iter()
            .filter(|e| matches!(&e.entry_type, 
                EntryType::Work { client, .. }
                    if client_filter.is_none() || client_filter.unwrap() == client))
            .map(|e| e.duration().unwrap_or(0.0))
            .sum()
    }

    // todo rewrite the report_str function to use this
    pub fn report(&self, client_filter: Option<&str>) -> Report {
        let mut result = Report{ total: self.total_work(client_filter), groups: vec![] };
        if result.total == 0.0 {
            return result;
        }

        for (client, task_duration) in self
            .entries
            .iter()
            .filter(|e| matches!(e.entry_type, EntryType::Work { .. }))
            .map(move |e| -> (&str, (&str, f64)) {
                match &e.entry_type {
                    EntryType::Work { client, task, ticket_id: _ } => (client, (task, e.duration().unwrap_or(0.0))),
                    _ => panic!("this shouldn't happen, we filtered already"),
                }
            })
            .filter(|(client, _)| client_filter.is_none() || &client_filter.unwrap() == client)
            .into_group_map()
        {
            let mut group = Group {client:  client.to_string(), total:task_duration.iter().map(|(_, duration)| duration).sum(),
                    entries: vec![]};
            for (task, durations) in task_duration.iter().cloned().into_group_map() {
                    group.entries.push(Task { desc: task.to_string(), total: durations.iter().cloned().sum::<f64>() });
            }
            result.groups.push(group);
        }
        result
    }

    pub fn report_str(&self, client_filter: Option<&str>) -> String {
        let mut result = String::with_capacity(100);

        let total = self.total_work(client_filter);
        if total == 0.0 {
            return result;
        }
        result.push_str(&format!("{}: {}\n", self.date.format("%A, %d %B"), total));

        for (client, task_duration) in self
            .entries
            .iter()
            .filter(|e| matches!(e.entry_type, EntryType::Work { .. }) && e.end.is_some())
            .map(move |e| -> (&str, (&str, f64)) {
                match &e.entry_type {
                    EntryType::Work { client, task, ticket_id: _ } => (client, (task, e.duration().unwrap_or(0.0))),
                    _ => panic!("this shouldn't happen, we filtered already"),
                }
            })
            .filter(|(client, _)| client_filter.is_none() || &client_filter.unwrap() == client)
            .into_group_map()
        {
            let client_total: f64 = task_duration.iter().map(|(_, duration)| duration).sum();
            result.push_str(&format!("  {client}:  {client_total}\n"));
            for (task, durations) in task_duration.iter().cloned().into_group_map() {
                result.push_str(&format!("    {}:  {}\n", task, durations.iter().cloned().sum::<f64>()));
            }
        }
        result.push('\n');
        result
    }
}
