use inquire::{Confirm, MultiSelect, Select, Text};
use std::collections::HashMap;

struct WorkTool {
    context: Vec<String>,
    filters: HashMap<String, String>,
    config: HashMap<String, HashMap<String, String>>,
}

impl WorkTool {
    fn new() -> Self {
        Self {
            context: vec!["Main Menu".to_string()],
            filters: HashMap::new(),
            config: HashMap::new(),
        }
    }

    fn get_context_display(&self) -> String {
        if self.context.len() == 1 {
            self.context[0].clone()
        } else {
            format!("{} > {}", self.context[0], self.context[1..].join(" > "))
        }
    }

    fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("sup");
        loop {
            match self.current_menu()? {
                MenuResult::Continue => continue,
                MenuResult::Exit => break,
                MenuResult::Back => {
                    if self.context.len() > 1 {
                        self.context.pop();
                    }
                }
            }
        }

        println!("Goodbye!");
        Ok(())
    }

    fn current_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let current = self.context.last().unwrap();

        match current.as_str() {
            "Main Menu" => self.main_menu(),
            "Configure" => self.configure_menu(),
            "Azure DevOps" | "Jira" | "GitHub" => self.service_menu(),
            "Filters" => self.filter_menu(),
            "View Data" => self.view_menu(),
            _ => Ok(MenuResult::Back),
        }
    }

    fn main_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let options = vec![
            "View Data",
            "Configure Services",
            "Set Filters",
            "Export Data",
            "Settings",
            "Exit",
        ];

        let selection = Select::new(
            &format!("[{}] Select option:", self.get_context_display()),
            options,
        )
        .prompt()?;

        match selection {
            "View Data" => {
                self.context.push("View Data".to_string());
                Ok(MenuResult::Continue)
            }
            "Configure Services" => {
                self.context.push("Configure".to_string());
                Ok(MenuResult::Continue)
            }
            "Set Filters" => {
                self.context.push("Filters".to_string());
                Ok(MenuResult::Continue)
            }
            "Export Data" => {
                self.export_data()?;
                Ok(MenuResult::Continue)
            }
            "Settings" => {
                println!("Settings menu would go here");
                Ok(MenuResult::Continue)
            }
            "Exit" => Ok(MenuResult::Exit),
            _ => Ok(MenuResult::Continue),
        }
    }

    fn configure_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let mut options = vec!["Azure DevOps", "Jira", "GitHub"];

        if !self.config.is_empty() {
            options.push("Show Configuration");
        }

        options.extend_from_slice(&["Test All Connections", "Back to Main Menu"]);

        let selection = Select::new(
            &format!("[{}] Select service:", self.get_context_display()),
            options,
        )
        .prompt()?;

        match selection {
            "Azure DevOps" | "Jira" | "GitHub" => {
                self.context.push(selection.to_string());
                Ok(MenuResult::Continue)
            }
            "Show Configuration" => {
                self.show_all_config();
                Ok(MenuResult::Continue)
            }
            "Test All Connections" => {
                self.test_all_connections();
                Ok(MenuResult::Continue)
            }
            "Back to Main Menu" => Ok(MenuResult::Back),
            _ => Ok(MenuResult::Continue),
        }
    }

    fn service_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let service = self.context.last().unwrap().clone();
        let options = vec![
            "Set URL",
            "Set Token/Password",
            "Set Organization/Project",
            "Test Connection",
            "Show Configuration",
            "Clear Configuration",
            "Back",
        ];

        let selection = Select::new(
            &format!("[{}] Configure {}:", self.get_context_display(), service),
            options,
        )
        .prompt()?;

        match selection {
            "Set URL" => {
                let url = Text::new("Enter URL:")
                    .with_placeholder("https://dev.azure.com/myorg")
                    .prompt()?;

                self.config
                    .entry(service.clone())
                    .or_default()
                    .insert("url".to_string(), url);

                println!("✅ URL configured for {}", service);
            }
            "Set Token/Password" => {
                let token = Text::new("Enter token/password:")
                    .with_placeholder("Your API token")
                    .prompt()?;

                self.config
                    .entry(service.clone())
                    .or_default()
                    .insert("token".to_string(), token);

                println!("✅ Token configured for {}", service);
            }
            "Set Organization/Project" => {
                let org = Text::new("Enter organization/project:").prompt()?;

                self.config
                    .entry(service.clone())
                    .or_default()
                    .insert("organization".to_string(), org);

                println!("✅ Organization configured for {}", service);
            }
            "Test Connection" => {
                self.test_connection(&service);
            }
            "Show Configuration" => {
                self.show_service_config(&service);
            }
            "Clear Configuration" => {
                if Confirm::new(&format!("Clear all configuration for {}?", service))
                    .with_default(false)
                    .prompt()?
                {
                    self.config.remove(&service);
                    println!("🗑️  Configuration cleared for {}", service);
                }
            }
            "Back" => return Ok(MenuResult::Back),
            _ => {}
        }

        Ok(MenuResult::Continue)
    }

    fn filter_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let options = vec![
            "Set Project Filter",
            "Set Person Filter",
            "Set Status Filter",
            "Set Date Range",
            "Show Active Filters",
            "Clear All Filters",
            "Back",
        ];

        let selection = Select::new(
            &format!("[{}] Filter Options:", self.get_context_display()),
            options,
        )
        .prompt()?;

        match selection {
            "Set Project Filter" => {
                // In a real app, you'd get this from your APIs
                let projects = vec!["Project A", "Project B", "Project C", "All Projects"];
                let project = Select::new("Select project:", projects).prompt()?;

                if project != "All Projects" {
                    self.filters
                        .insert("project".to_string(), project.to_string());
                    println!("✅ Project filter set to: {}", project);
                } else {
                    self.filters.remove("project");
                    println!("✅ Project filter cleared");
                }
            }
            "Set Person Filter" => {
                let person = Text::new("Enter person name or email:").prompt()?;

                if !person.trim().is_empty() {
                    self.filters.insert("person".to_string(), person);
                    println!("✅ Person filter set");
                }
            }
            "Set Status Filter" => {
                let statuses = vec!["To Do", "In Progress", "Done", "Blocked", "All Statuses"];
                let status_options =
                    MultiSelect::new("Select statuses (space to select):", statuses).prompt()?;

                if !status_options.is_empty() && !status_options.contains(&"All Statuses") {
                    self.filters
                        .insert("status".to_string(), status_options.join(", "));
                    println!("✅ Status filters set");
                } else {
                    self.filters.remove("status");
                    println!("✅ Status filter cleared");
                }
            }
            "Set Date Range" => {
                let ranges = vec![
                    "Last 7 days",
                    "Last 30 days",
                    "Last 90 days",
                    "Custom",
                    "No filter",
                ];
                let range = Select::new("Select date range:", ranges).prompt()?;

                match range {
                    "Custom" => {
                        let start = Text::new("Start date (YYYY-MM-DD):")
                            .with_placeholder("2024-01-01")
                            .prompt()?;
                        let end = Text::new("End date (YYYY-MM-DD):")
                            .with_placeholder("2024-12-31")
                            .prompt()?;

                        self.filters
                            .insert("date_range".to_string(), format!("{} to {}", start, end));
                    }
                    "No filter" => {
                        self.filters.remove("date_range");
                    }
                    _ => {
                        self.filters
                            .insert("date_range".to_string(), range.to_string());
                    }
                }
                println!("✅ Date range filter updated");
            }
            "Show Active Filters" => {
                self.show_filters();
            }
            "Clear All Filters" => {
                if Confirm::new("Clear all filters?")
                    .with_default(false)
                    .prompt()?
                {
                    self.filters.clear();
                    println!("🗑️  All filters cleared");
                }
            }
            "Back" => return Ok(MenuResult::Back),
            _ => {}
        }

        Ok(MenuResult::Continue)
    }

    fn view_menu(&mut self) -> Result<MenuResult, Box<dyn std::error::Error>> {
        let options = vec![
            "Dashboard View",
            "Table View",
            "Card View",
            "By Project",
            "By Person",
            "Export Current View",
            "Refresh Data",
            "Back",
        ];

        let selection = Select::new(
            &format!("[{}] View Data:", self.get_context_display()),
            options,
        )
        .prompt()?;

        match selection {
            "Dashboard View" | "Table View" | "Card View" | "By Project" | "By Person" => {
                println!("\n🚀 Launching {} with current filters...", selection);
                self.show_filters();
                println!(
                    "(This would launch ratatui with {} layout)",
                    selection.to_lowercase()
                );
                println!("Press Enter to continue...");
                std::io::stdin().read_line(&mut String::new())?;
            }
            "Export Current View" => {
                self.export_data()?;
            }
            "Refresh Data" => {
                println!("🔄 Refreshing data from all configured services...");
                println!("(This would fetch fresh data)");
            }
            "Back" => return Ok(MenuResult::Back),
            _ => {}
        }

        Ok(MenuResult::Continue)
    }

    fn show_filters(&self) {
        if self.filters.is_empty() {
            println!("📋 No active filters");
        } else {
            println!("📋 Active filters:");
            for (key, value) in &self.filters {
                println!("   {}: {}", key, value);
            }
        }
    }

    fn show_service_config(&self, service: &str) {
        if let Some(config) = self.config.get(service) {
            println!("⚙️  {} Configuration:", service);
            for (key, value) in config {
                if key == "token" {
                    println!("   {}: {}", key, "*".repeat(value.len().min(8)));
                } else {
                    println!("   {}: {}", key, value);
                }
            }
        } else {
            println!("❌ No configuration found for {}", service);
        }
    }

    fn show_all_config(&self) {
        println!("⚙️  All Service Configurations:");
        for (service, config) in &self.config {
            println!("\n  {}:", service);
            for (key, value) in config {
                if key == "token" {
                    println!("     {}: {}", key, "*".repeat(value.len().min(8)));
                } else {
                    println!("     {}: {}", key, value);
                }
            }
        }
        if self.config.is_empty() {
            println!("   No services configured yet");
        }
    }

    fn test_connection(&self, service: &str) {
        println!("🔌 Testing {} connection...", service);
        if let Some(config) = self.config.get(service) {
            if config.contains_key("url") && config.contains_key("token") {
                println!("✅ Connection test passed for {}", service);
                println!("   (This would actually test the connection)");
            } else {
                println!("❌ Missing required configuration for {}", service);
                println!("   Need: URL and Token/Password");
            }
        } else {
            println!("❌ No configuration found for {}", service);
        }
    }

    fn test_all_connections(&self) {
        println!("🔌 Testing all configured connections...");
        for service in self.config.keys() {
            self.test_connection(service);
        }
        if self.config.is_empty() {
            println!("❌ No services configured");
        }
    }

    fn export_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        let formats = vec!["CSV", "JSON", "Excel", "PDF Report"];
        let format = Select::new("Export format:", formats).prompt()?;

        let filename = Text::new("Filename:")
            .with_placeholder(&format!("work_items.{}", format.to_lowercase()))
            .prompt()?;

        println!("📤 Exporting data as {} to '{}'", format, filename);
        println!("   Filters applied: {:?}", self.filters);
        println!("   (Export would happen here)");

        Ok(())
    }
}

enum MenuResult {
    Continue,
    Back,
    Exit,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tool = WorkTool::new();
    tool.run()
}
