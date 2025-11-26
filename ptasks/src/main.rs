use anyhow::{Context, Result};
use inquire::{Select, Text};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::thread;

static INPUT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{input:([^}]+)\}").unwrap());

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
    inputs: Option<Vec<Input>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(rename = "type")]
    input_type: String,
    id: String,
    description: Option<String>,
    options: Option<Vec<String>>,
    // TODO how is default supposed to work?
    // default: Option<String>,
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
                        execute_task(&dep_task, &all_tasks_clone, &all_inputs_clone)
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

    // TODO: tasks _can_ not have a command, this is wrong
    // it's other types like npm or typescript
    // problemMatchers etc
    let mut input_values: HashMap<String, String> = HashMap::new();

    let mut strings_to_scan: Vec<&str> = Vec::new();
    if let Some(command) = &task.command {
        strings_to_scan.push(command);
    }
    if let Some(args) = &task.args {
        strings_to_scan.extend(args.iter().map(|s| s.as_str()));
    }

    for text in strings_to_scan {
        for caps in INPUT_RE.captures_iter(text) {
            let var_name = caps.get(1).unwrap().as_str();

            if !input_values.contains_key(var_name) {
                let input = all_inputs
                    .get(var_name)
                    .unwrap_or_else(|| panic!("input {var_name} not found in input array"));

                let prompt = match &input.description {
                    Some(p) => p,
                    None => &format!("Enter value for {}", input.id),
                };
                let value = match input.input_type.as_str() {
                    "promptString" => Text::new(prompt)
                        .prompt()
                        .context("User cancelled input prompt")?,
                    "pickString" => {
                        let options = input
                            .options
                            .to_owned()
                            .expect("pickString input should have options");
                        Select::new(prompt, options).prompt()?
                    }
                    "command" => unimplemented!("input type not supported"),
                    _ => unimplemented!("input type not supported"),
                };

                input_values.insert(var_name.to_string(), value);
            }
        }
    }

    let replacer = |text: &str| {
        INPUT_RE
            .replace_all(text, |caps: &regex::Captures| {
                let var_name = &caps[1];
                input_values.get(var_name).unwrap()
            })
            .to_string()
    };

    let command_name = task
        .command
        .as_ref()
        .map(|s| replacer(s))
        .context("Task has no command to execute")?;

    let final_args = task
        .args
        .as_ref()
        .map(|args| args.iter().map(|arg| replacer(arg)).collect::<Vec<_>>());

    let mut command = match task.task_type.as_deref() {
        Some("process") => {
            let mut cmd = Command::new(&command_name);
            if let Some(args) = &final_args {
                cmd.args(args);
            }
            cmd
        }
        _ => {
            // v****e defaults to "shell"
            let mut script = command_name.clone();
            if let Some(args) = &final_args {
                for i in 1..=args.len() {
                    script.push_str(&format!(" \"${i}\""));
                }
            }

            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&script);
            cmd.arg(&command_name);
            if let Some(args) = &final_args {
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

    let input_map: HashMap<String, Input> = match tasks_file.inputs {
        Some(inputs) => inputs
            .into_iter()
            .map(|inp| (inp.id.clone(), inp))
            .collect(),
        None => HashMap::default(),
    };

    let task_labels: Vec<Task> = task_map.values().cloned().collect();

    let selected_task = Select::new("Select a task to run", task_labels).prompt()?;

    if let Err(e) = execute_task(&selected_task, &task_map, &input_map) {
        eprintln!("\nError running task '{}':\n{:#?}", selected_task.label, e);
        std::process::exit(1);
    }

    Ok(())
}
