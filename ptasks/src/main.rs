use anyhow::{Context, Result};
use inquire::Select;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Task {
    label: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    options: Option<TaskOptions>,
}

impl std::fmt::Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TaskOptions {
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TasksFile {
    tasks: Vec<Task>,
}

fn read_tasks_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Task>> {
    let file_content = fs::read_to_string(path).context("Failed to read tasks.json")?;
    let tasks_file: TasksFile =
        json5::from_str(&file_content).context("Failed to parse tasks.json")?;
    Ok(tasks_file.tasks)
}

fn execute_task(task: &Task) -> Result<()> {
    set_window_title(&task.label)?;

    let command_name = task
        .command
        .as_ref()
        .context("Task has no command to execute")?;
    let mut command = Command::new(command_name);

    if let Some(args) = &task.args {
        command.args(args);
    }

    if let Some(options) = &task.options {
        if let Some(cwd) = &options.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &options.env {
            command.envs(env);
        }
    }

    let mut child = command
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to spawn command: {}", command_name))?;

    let status = child.wait().context("Failed to wait for command")?;

    if !status.success() {
        anyhow::bail!("Command failed with status: {}", status);
    }

    set_window_title("noot noot")?;
    Ok(())
}

fn set_window_title(title: &str) -> io::Result<()> {
    print!("\u{1b}]0;{title}\u{7}");
    io::stdout().flush()
}

fn main() -> Result<()> {
    let tasks_json_path = ".vscode/tasks.json";
    let tasks = read_tasks_from_file(tasks_json_path)?;

    let task = Select::new("Select a task to run", tasks).prompt()?;
    execute_task(&task)
}
