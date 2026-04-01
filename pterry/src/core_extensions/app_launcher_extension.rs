use crate::extension_trait::{Extension, ExtensionError, ExtensionItem, ExtensionMetadata};
use crate::fuzzy::fuzzy_score;
use crate::modes;
use async_trait::async_trait;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct AppEntry {
    name: String,
    exec: String,
    icon: Option<String>,
    comment: Option<String>,
}

pub struct AppLauncherExtension {
    metadata: ExtensionMetadata,
    apps: Arc<Mutex<Vec<AppEntry>>>,
}

impl fmt::Debug for AppLauncherExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppLauncherExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for AppLauncherExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl AppLauncherExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            name: modes::APP_LAUNCHER.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Launch installed applications".to_string()),
            author: None,
            language: crate::extension_trait::ExtensionLanguage::Rust,
            entry_point: "app_launcher_extension.rs".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        let apps = discover_apps();

        Self {
            metadata,
            apps: Arc::new(Mutex::new(apps)),
        }
    }
}

#[cfg(target_os = "linux")]
fn discover_apps() -> Vec<AppEntry> {
    let mut dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
    }

    // Add XDG_DATA_DIRS entries
    if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':') {
            let app_dir = PathBuf::from(dir).join("applications");
            if !dirs.contains(&app_dir) {
                dirs.push(app_dir);
            }
        }
    }

    let mut apps = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for dir in &dirs {
        if !dir.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(app) = parse_desktop_file(&path)
                && !seen_names.contains(&app.name)
            {
                seen_names.insert(app.name.clone());
                apps.push(app);
            }
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

#[cfg(not(target_os = "linux"))]
fn discover_apps() -> Vec<AppEntry> {
    // Stub for non-Linux platforms
    Vec::new()
}

#[cfg(target_os = "linux")]
fn parse_desktop_file(path: &std::path::Path) -> Option<AppEntry> {
    let content = fs::read_to_string(path).ok()?;

    let mut in_desktop_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut comment = None;
    let mut app_type = None;
    let mut no_display = false;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some(val) = line.strip_prefix("Name=") {
            if name.is_none() {
                name = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("Exec=") {
            exec = Some(clean_exec(val));
        } else if let Some(val) = line.strip_prefix("Icon=") {
            icon = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("Comment=") {
            if comment.is_none() {
                comment = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("Type=") {
            app_type = Some(val.to_string());
        } else if line == "NoDisplay=true" {
            no_display = true;
        }
    }

    // Only include Application type entries that aren't hidden
    if app_type.as_deref() != Some("Application") || no_display {
        return None;
    }

    Some(AppEntry {
        name: name?,
        exec: exec?,
        icon,
        comment,
    })
}

/// Strip field codes (%F, %U, %f, %u, etc.) from Exec values
fn clean_exec(exec: &str) -> String {
    let mut result = String::new();
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Skip the field code character
            chars.next();
        } else {
            result.push(ch);
        }
    }
    result.trim().to_string()
}

#[async_trait]
impl Extension for AppLauncherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let apps = self.apps.lock().unwrap_or_else(|e| e.into_inner());

        if query.is_empty() {
            // Return all apps (capped to avoid overwhelming the UI)
            let items: Vec<ExtensionItem> = apps
                .iter()
                .take(50)
                .map(|app| ExtensionItem {
                    title: app.name.clone(),
                    subtitle: app.comment.clone(),
                    icon: app.icon.clone(),
                    action: format!("launch-app:{}", app.exec),
                    id: Some(format!("app-{}", app.name)),
                    detail: None,
                    accessories: vec![],
                    extra_actions: vec![],
                    detail_metadata: vec![],
                    thumbnail_rgba: None,
                    grid_columns: None,
                })
                .collect();
            return Ok(items);
        }

        let mut scored: Vec<(i32, &AppEntry)> = apps
            .iter()
            .map(|app| {
                let name_score = fuzzy_score(query, &app.name);
                let comment_score = app
                    .comment
                    .as_ref()
                    .map(|c| fuzzy_score(query, c))
                    .unwrap_or(0);
                (name_score.max(comment_score), app)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));

        let items: Vec<ExtensionItem> = scored
            .into_iter()
            .take(50)
            .map(|(_, app)| ExtensionItem {
                title: app.name.clone(),
                subtitle: app.comment.clone(),
                icon: app.icon.clone(),
                action: format!("launch-app:{}", app.exec),
                id: Some(format!("app-{}", app.name)),
                detail: None,
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            })
            .collect();

        Ok(items)
    }

    async fn on_action(&self, _action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        // Actions are handled by app.rs via the launch-app: prefix
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a launcher extension pre-loaded with a fixed set of apps.
    fn ext_with_apps(apps: Vec<AppEntry>) -> AppLauncherExtension {
        AppLauncherExtension {
            metadata: ExtensionMetadata {
                name: modes::APP_LAUNCHER.to_string(),
                version: "1.0.0".to_string(),
                description: None,
                author: None,
                language: crate::extension_trait::ExtensionLanguage::Rust,
                entry_point: "app_launcher_extension.rs".to_string(),
                permissions: vec![],
                auto_load: true,
                title: None,
                preferences: vec![],
                is_development: false,
            },
            apps: Arc::new(Mutex::new(apps)),
        }
    }

    // ── clean_exec ────────────────────────────────────────────────────────────

    #[test]
    fn clean_exec_strips_single_field_code() {
        assert_eq!(clean_exec("firefox %U"), "firefox");
    }

    #[test]
    fn clean_exec_strips_multiple_field_codes() {
        assert_eq!(clean_exec("app --arg %f %F"), "app --arg");
    }

    #[test]
    fn clean_exec_no_codes_unchanged() {
        assert_eq!(clean_exec("firefox --new-window"), "firefox --new-window");
    }

    #[test]
    fn clean_exec_strips_lowercase_field_codes() {
        assert_eq!(clean_exec("gimp %u"), "gimp");
    }

    // ── on_search — empty query ───────────────────────────────────────────────

    #[tokio::test]
    async fn empty_query_returns_all_apps() {
        let ext = ext_with_apps(vec![
            AppEntry {
                name: "Alpha".to_string(),
                exec: "alpha".to_string(),
                icon: None,
                comment: None,
            },
            AppEntry {
                name: "Beta".to_string(),
                exec: "beta".to_string(),
                icon: None,
                comment: None,
            },
        ]);
        let results = ext.on_search("").await.expect("should not error");
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn empty_query_capped_at_fifty() {
        let apps: Vec<AppEntry> = (0..60)
            .map(|i| AppEntry {
                name: format!("App{i:02}"),
                exec: format!("app{i}"),
                icon: None,
                comment: None,
            })
            .collect();
        let ext = ext_with_apps(apps);
        let results = ext.on_search("").await.expect("should not error");
        assert_eq!(
            results.len(),
            50,
            "empty-query results should be capped at 50"
        );
    }

    // ── on_search — fuzzy matching ────────────────────────────────────────────

    #[tokio::test]
    async fn fuzzy_query_filters_out_non_matching_apps() {
        let ext = ext_with_apps(vec![
            AppEntry {
                name: "Firefox".to_string(),
                exec: "firefox".to_string(),
                icon: None,
                comment: None,
            },
            AppEntry {
                name: "Calculator".to_string(),
                exec: "calc".to_string(),
                icon: None,
                comment: None,
            },
        ]);
        let results = ext.on_search("fire").await.expect("should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Firefox");
    }

    #[tokio::test]
    async fn fuzzy_query_no_match_returns_empty() {
        let ext = ext_with_apps(vec![AppEntry {
            name: "Firefox".to_string(),
            exec: "firefox".to_string(),
            icon: None,
            comment: None,
        }]);
        let results = ext.on_search("zzz").await.expect("should not error");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fuzzy_query_also_matches_comment() {
        let ext = ext_with_apps(vec![
            AppEntry {
                name: "TextEditor".to_string(),
                exec: "gedit".to_string(),
                icon: None,
                comment: Some("Edit files".to_string()),
            },
            AppEntry {
                name: "Browser".to_string(),
                exec: "firefox".to_string(),
                icon: None,
                comment: None,
            },
        ]);
        // "edit" matches TextEditor's comment but not Browser's name
        let results = ext.on_search("edit").await.expect("should not error");
        assert!(
            results.iter().any(|r| r.title == "TextEditor"),
            "should match via comment"
        );
    }

    #[tokio::test]
    async fn fuzzy_results_have_action_prefix() {
        let ext = ext_with_apps(vec![AppEntry {
            name: "Firefox".to_string(),
            exec: "/usr/bin/firefox".to_string(),
            icon: None,
            comment: None,
        }]);
        let results = ext.on_search("fox").await.expect("should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "launch-app:/usr/bin/firefox");
    }

    // ── no detail panel for app launcher ─────────────────────────────────────

    /// App launcher items must have no detail content so the detail panel
    /// never opens when browsing apps (Raycast itself doesn't show a preview
    /// pane for the app launcher).
    #[tokio::test]
    async fn items_have_no_detail() {
        let ext = ext_with_apps(vec![AppEntry {
            name: "MyApp".to_string(),
            exec: "myapp --run".to_string(),
            icon: None,
            comment: Some("A great app".to_string()),
        }]);
        let results = ext.on_search("MyApp").await.expect("should not error");
        assert_eq!(results.len(), 1);
        assert!(
            results[0].detail.is_none(),
            "app launcher items must not have detail content"
        );
    }

    // ── parse_desktop_file ────────────────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_desktop_file_returns_app_entry_for_valid_file() {
        let content = "[Desktop Entry]\nType=Application\nName=TestApp\nExec=testapp %U\nIcon=testapp\nComment=A test app\n";
        let path = std::env::temp_dir().join("test_valid.desktop");
        std::fs::write(&path, content).expect("write temp desktop file");
        let entry = parse_desktop_file(&path).expect("should parse");
        assert_eq!(entry.name, "TestApp");
        assert_eq!(entry.exec, "testapp");
        assert_eq!(entry.icon.as_deref(), Some("testapp"));
        assert_eq!(entry.comment.as_deref(), Some("A test app"));
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_desktop_file_rejects_nodisplay_entries() {
        let content =
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nNoDisplay=true\n";
        let path = std::env::temp_dir().join("test_nodisplay.desktop");
        std::fs::write(&path, content).expect("write temp desktop file");
        let entry = parse_desktop_file(&path);
        assert!(
            entry.is_none(),
            "NoDisplay=true entries should be filtered out"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_desktop_file_rejects_non_application_type() {
        let content = "[Desktop Entry]\nType=Link\nName=MyLink\nExec=link\n";
        let path = std::env::temp_dir().join("test_link_type.desktop");
        std::fs::write(&path, content).expect("write temp desktop file");
        let entry = parse_desktop_file(&path);
        assert!(
            entry.is_none(),
            "non-Application types should be filtered out"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_desktop_file_ignores_sections_after_desktop_entry() {
        let content = "[Desktop Entry]\nType=Application\nName=App\nExec=app\n[OtherSection]\nName=ShouldNotOverride\n";
        let path = std::env::temp_dir().join("test_sections.desktop");
        std::fs::write(&path, content).expect("write temp desktop file");
        let entry = parse_desktop_file(&path).expect("should parse");
        assert_eq!(
            entry.name, "App",
            "name from [OtherSection] should not override [Desktop Entry]"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ── existing poison-mutex test ────────────────────────────────────────────

    #[tokio::test]
    async fn on_search_survives_poisoned_mutex() {
        let ext = AppLauncherExtension::new();

        // Poison the mutex by panicking while holding the lock.
        let apps_clone = Arc::clone(&ext.apps);
        let _ = std::thread::spawn(move || {
            let _guard = apps_clone.lock().unwrap();
            panic!("intentional poison");
        })
        .join();

        // on_search must not panic on a poisoned mutex.
        let result = ext.on_search("").await;
        assert!(result.is_ok());
    }
}
