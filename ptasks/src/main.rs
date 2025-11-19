use anyhow::{Context, Result};
use inquire::{Select, Text};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Task {
    label: String,
    #[serde(rename = "type")]
    task_type: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    options: Option<TaskOptions>,
    depends_on: Option<Vec<String>>,
    depends_order: Option<String>,
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

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TasksFile {
    tasks: Vec<Task>,
    inputs: Vec<Input>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(rename = "type")]
    input_type: String,
    id: String,
    description: Option<String>,
    options: Option<Vec<String>>,
    default: Option<String>,
}

fn read_tasks_file<P: AsRef<Path>>(path: P) -> Result<TasksFile> {
    let file_content = fs::read_to_string(path).context("Failed to read tasks.json")?;
    let current_working_dir = std::env::current_dir().context("Failed to get current dir")?;
    let cwd_str = current_working_dir.to_string_lossy();
    // TODO this can include relative paths, more work needed
    // e.g. ${workspaceFolder:src} should be cwd/src
    let file_content = file_content.replace("${workspaceFolder}", &cwd_str);
    let tasks_file: TasksFile =
        json5::from_str(&file_content).context("Failed to parse tasks.json")?;
    Ok(tasks_file)
}

fn execute_task(
    task: &Task,
    all_tasks: &HashMap<String, Task>,
    all_inputs: &HashMap<String, Input>,
) -> Result<()> {
    if let Some(dependencies) = &task.depends_on {
        match task.depends_order.as_deref() {
            Some("sequence") => {
                for dep_label in dependencies {
                    let dep_task = all_tasks
                        .get(dep_label)
                        .context(format!("Dependent task '{dep_label}' not found"))?;
                    execute_task(dep_task, all_tasks, all_inputs)?;
                }
            }
            Some("parallel") => {
                let mut handles = vec![];
                for dep_label in dependencies {
                    let dep_task = all_tasks
                        .get(dep_label)
                        .context(format!("Dependent task '{dep_label}' not found"))?
                        .clone();
                    let all_tasks_clone = all_tasks.to_owned();
                    let all_inputs_clone = all_inputs.to_owned();

                    let handle = thread::spawn(move || {
                        let result = execute_task(&dep_task, &all_tasks_clone, &all_inputs_clone);
                        result
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap()?; // Wait for each thread to complete and propagate errors
                }
            }
            _ => {
                // Default to sequence if dependsOrder is not specified or unknown
                for dep_label in dependencies {
                    let dep_task = all_tasks
                        .get(dep_label)
                        .context(format!("Dependent task '{dep_label}' not found"))?;
                    println!("\n--> Running dependent task: {dep_label}");
                    execute_task(dep_task, all_tasks, all_inputs)?;
                    println!("\n<-- Finished dependent task: {dep_label}");
                }
            }
        }
    }

    set_window_title(&task.label)?;

    // TODO: tasks can _not_ have a command, this is wrong
    // it's other types like npm or typescript
    // problemMatchers etc
    let raw_command = task
        .command
        .as_ref()
        .context("Task has no command to execute")?;
    // TODO: lazy static or oncelock
    let re = Regex::new(r"\$\{input:([^}]+)\}").unwrap();

    let mut input_values: HashMap<String, String> = HashMap::new();

    for caps in re.captures_iter(raw_command) {
        let var_name = caps.get(1).unwrap().as_str();

        if !input_values.contains_key(var_name) {
            // TODO: we should be checking for the type of input (prompt)
            // not just assume
            // pick list we can do easily, too
            println!("> Input required for task '{}'", task.label);
            let value = Text::new(&format!("Enter value for '{}':", var_name))
                .prompt()
                .context("User cancelled input prompt")?;

            input_values.insert(var_name.to_string(), value);
        }
    }

    let command_name = re
        .replace_all(raw_command, |caps: &regex::Captures| {
            let var_name = &caps[1];
            input_values.get(var_name).unwrap()
        })
        .to_string();

    let mut command = match task.task_type.as_deref() {
        Some("process") => {
            let mut cmd = Command::new(&command_name);
            if let Some(args) = &task.args {
                cmd.args(args);
            }
            cmd
        }
        _ => {
            // v****e defaults to "shell"
            let mut script = command_name.clone();
            if let Some(args) = &task.args {
                for i in 1..=args.len() {
                    script.push_str(&format!(" \"${i}\""));
                }
            }

            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&script);
            cmd.arg(&command_name);
            if let Some(args) = &task.args {
                cmd.args(args);
            }
            cmd
        }
    };

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
        .with_context(|| {
            format!(
                "Failed to spawn command: {command_name} with {:?}",
                &task.options
            )
        })?;

    let status = child.wait().context("Failed to wait for command")?;

    if !status.success() {
        anyhow::bail!("Command failed with status: {status}");
    }

    set_window_title("ptasks")?;
    Ok(())
}

fn set_window_title(title: &str) -> io::Result<()> {
    print!("\u{1b}]0;{title}\u{7}");
    io::stdout().flush()
}

fn main() -> Result<()> {
    let tasks_json_path = ".vscode/tasks.json";
    let tasks_file = read_tasks_file(tasks_json_path)?;

    let task_map: HashMap<String, Task> = tasks_file
        .tasks
        .into_iter()
        .map(|task| (task.label.clone(), task))
        .collect();

    let input_map: HashMap<String, Input> = tasks_file
        .inputs
        .into_iter()
        .map(|inp| (inp.id.clone(), inp))
        .collect();

    let task_labels: Vec<Task> = task_map.values().cloned().collect();

    let selected_task = Select::new("Select a task to run", task_labels).prompt()?;

    if let Err(e) = execute_task(&selected_task, &task_map, &input_map) {
        eprintln!("\nError running task '{}':\n{:#?}", selected_task.label, e);
        std::process::exit(1);
    }

    Ok(())
}
