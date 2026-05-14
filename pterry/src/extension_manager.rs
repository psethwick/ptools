use crate::clipboard_manager::ClipboardManager;
use crate::core_extensions::app_launcher_extension::AppLauncherExtension;
use crate::core_extensions::window_switcher_extension::WindowSwitcherExtension;
use crate::core_extensions::calculator_extension::CalculatorExtension;
use crate::core_extensions::clipboard_extension::ClipboardExtension;
use crate::core_extensions::settings_extension::{
    ExtensionPrefsExtension, SetGitHubTokenExtension, SettingsExtension,
};
use crate::core_extensions::store_extension::StoreExtension;
use crate::core_extensions::store_native_extension::StoreNativeExtension;
use crate::core_extensions::store_raycast_extension::StoreRaycastExtension;
use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata,
};
use crate::js_extension::JsExtension;
use crate::modes;
use crate::transpiler::{is_raycast_api_extension, transpile_for_raycast, transpile_typescript};
use crossbeam_channel::{Receiver, Sender};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub enum ExtensionMessage {
    ExtensionLoaded(String),
    ExtensionError(String, String),
    SearchResults(String, Vec<ExtensionItem>),
    ExtensionUnloaded(String),
    /// style is one of "success" | "failure" | "animated"
    ShowToast(String, String, String),
    /// Sent after `on_action` completes for a named extension.
    /// The UI should re-run search so any navigation push/pop or state change
    /// that happened inside the action handler becomes visible immediately.
    ActionComplete(String),
    /// Sent by a JS extension when its navigation stack changes.
    /// Payload is `"push-view:<title>"` or `"pop-view"`.
    Navigate(String),
    /// Sent by a JS extension calling `raycast.hideWindow()`.
    /// The launcher window should become invisible.
    HideWindow,
}

pub struct ExtensionManager {
    extensions: Arc<RwLock<HashMap<String, Arc<dyn Extension>>>>,
    sender: Sender<ExtensionMessage>,
    receiver: Receiver<ExtensionMessage>,
    clipboard_manager: Option<Arc<ClipboardManager>>,
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionManager {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();

        Self {
            extensions: Arc::new(RwLock::new(HashMap::new())),
            sender,
            receiver,
            clipboard_manager: None,
        }
    }

    pub fn with_clipboard_manager(mut self, clipboard_manager: Arc<ClipboardManager>) -> Self {
        self.clipboard_manager = Some(clipboard_manager);
        self
    }

    pub async fn load_extension(
        &self,
        path: PathBuf,
        name: Option<String>,
    ) -> Result<(), ExtensionError> {
        let extension_name = name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        // Load metadata if it exists
        let mut metadata = self.load_extension_metadata(&path, &extension_name)?;

        // Mark development extensions: anything NOT under ~/.pterry/extensions/
        // is considered a dev-mode load (e.g. loaded from ./extensions/).
        let user_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".pterry")
            .join("extensions");
        metadata.is_development = !path.starts_with(&user_dir);

        // Create the appropriate extension based on language
        let extension: Arc<dyn Extension> = match metadata.language {
            ExtensionLanguage::JavaScript | ExtensionLanguage::TypeScript => {
                let file_path_str = path.to_str().unwrap_or("unknown");
                let raw_code = fs::read_to_string(&path).map_err(|e| {
                    ExtensionError::LoadError(format!("Failed to read JS file: {e}"))
                })?;

                // Detect Raycast API extensions: they import from "@raycast/api"
                let is_raycast = is_raycast_api_extension(&raw_code);

                let (js_code, raycast_cjs_mode) = if is_raycast {
                    // Full Raycast-compat transform: TS + JSX + CommonJS output
                    let cjs = transpile_for_raycast(&raw_code, file_path_str)?;
                    (cjs, true)
                } else if metadata.language == ExtensionLanguage::TypeScript {
                    // Plain TypeScript transpilation (no module transform)
                    let js = transpile_typescript(&raw_code, file_path_str)?;
                    (js, false)
                } else {
                    (raw_code, false)
                };

                Arc::new(
                    JsExtension::new_with_sender_and_clipboard(
                        metadata.clone(),
                        &js_code,
                        raycast_cjs_mode,
                        self.sender.clone(),
                        self.clipboard_manager.clone(),
                    )
                    .await?,
                )
            }
            ExtensionLanguage::Rust => {
                return Err(ExtensionError::LoadError(
                    "Rust extensions are built-in and cannot be runtime loaded".to_string(),
                ));
            }
        };

        // Store the extension
        self.extensions
            .write()
            .await
            .insert(extension_name.clone(), extension);

        // Send loaded notification
        let _ = self
            .sender
            .send(ExtensionMessage::ExtensionLoaded(extension_name));

        Ok(())
    }

    pub async fn load_builtin_extensions(&self) -> Result<(), ExtensionError> {
        // Load built-in Rust extensions
        if let Some(ref clipboard_manager) = self.clipboard_manager {
            let clipboard_extension = Arc::new(ClipboardExtension::new(clipboard_manager.clone()));
            self.extensions
                .write()
                .await
                .insert(modes::CLIPBOARD_HISTORY.to_string(), clipboard_extension);
            let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
                modes::CLIPBOARD_HISTORY.to_string(),
            ));
        }

        let app_launcher = Arc::new(AppLauncherExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::APP_LAUNCHER.to_string(), app_launcher);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::APP_LAUNCHER.to_string(),
        ));

        let window_switcher = Arc::new(WindowSwitcherExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::WINDOW_SWITCHER.to_string(), window_switcher);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::WINDOW_SWITCHER.to_string(),
        ));

        let calculator = Arc::new(CalculatorExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::CALCULATOR.to_string(), calculator);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::CALCULATOR.to_string(),
        ));

        let store = Arc::new(StoreExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::STORE.to_string(), store);
        let _ = self
            .sender
            .send(ExtensionMessage::ExtensionLoaded(modes::STORE.to_string()));

        let store_native = Arc::new(StoreNativeExtension::new_with_sender(self.sender.clone()));
        self.extensions
            .write()
            .await
            .insert(modes::STORE_NATIVE.to_string(), store_native);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::STORE_NATIVE.to_string(),
        ));

        let store_raycast = Arc::new(StoreRaycastExtension::new_with_sender(self.sender.clone()));
        self.extensions
            .write()
            .await
            .insert(modes::STORE_RAYCAST.to_string(), store_raycast);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::STORE_RAYCAST.to_string(),
        ));

        let settings = Arc::new(SettingsExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::SETTINGS.to_string(), settings);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::SETTINGS.to_string(),
        ));

        let set_github_token = Arc::new(SetGitHubTokenExtension::new());
        self.extensions.write().await.insert(
            modes::SETTINGS_SET_GITHUB_TOKEN.to_string(),
            set_github_token,
        );
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::SETTINGS_SET_GITHUB_TOKEN.to_string(),
        ));

        let ext_prefs = Arc::new(ExtensionPrefsExtension::new());
        self.extensions
            .write()
            .await
            .insert(modes::SETTINGS_EXT_PREFS.to_string(), ext_prefs);
        let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(
            modes::SETTINGS_EXT_PREFS.to_string(),
        ));

        Ok(())
    }

    fn load_extension_metadata(
        &self,
        path: &Path,
        name: &str,
    ) -> Result<ExtensionMetadata, ExtensionError> {
        let ext_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
        let parent = path.parent().unwrap_or(Path::new("."));

        // ── Sidecar JSON (partial overrides) ─────────────────────────────────
        // When a <name>.json file sits next to <name>.js/.ts (e.g. calculator.json
        // next to calculator.js), treat it as a set of overrides to merge into
        // the default metadata. This lets extensions opt out of auto-load without
        // requiring a full extension.json.
        //
        // NOTE: this is NOT extension.json — that file takes precedence below.
        let sidecar_path = parent.join(format!("{ext_stem}.json"));
        let overrides: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(&sidecar_path).unwrap_or_default())
                .unwrap_or_default();

        // ── extension.json (complete definition) ──────────────────────────────
        // A full metadata file in the same directory as the entry point.
        // Takes priority over any sidecar.
        let metadata_path = parent.join("extension.json");
        if metadata_path.exists() {
            let metadata_content = fs::read_to_string(&metadata_path).map_err(|e| {
                ExtensionError::LoadError(format!("Failed to read metadata: {e}"))
            })?;

            let mut meta: ExtensionMetadata = serde_json::from_str(&metadata_content)
                .map_err(|e| ExtensionError::LoadError(format!("Invalid metadata JSON: {e}")))?;

            // Merge sidecar overrides on top of the full definition (allows
            // extension.json to be a template with just the sidecar flipping
            // a field or two).
            apply_overrides(&mut meta, &overrides);
            return Ok(meta);
        }

        // ── No extension.json: build defaults and apply sidecar overrides ─────
        let language = match path.extension().and_then(|s| s.to_str()) {
            Some("js") => ExtensionLanguage::JavaScript,
            Some("ts" | "tsx") => ExtensionLanguage::TypeScript,
            _ => {
                return Err(ExtensionError::LoadError(
                    "Unknown extension type".to_string(),
                ));
            }
        };

        let mut metadata = ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: Some(format!("Extension: {name}")),
            author: None,
            language,
            entry_point: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false, // overwritten below by load_extension()
        };

        apply_overrides(&mut metadata, &overrides);

        Ok(metadata)
    }
}

/// Merge sidecar `overrides` into `meta`. Only fields present in `overrides`
/// replace the defaults; everything else stays unchanged.
fn apply_overrides(meta: &mut ExtensionMetadata, overrides: &serde_json::Map<String, serde_json::Value>) {
    if let Some(al) = overrides.get("auto_load").and_then(|v| v.as_bool()) {
        meta.auto_load = al;
    }
    if let Some(desc) = overrides.get("description").and_then(|v| v.as_str()) {
        meta.description = Some(desc.to_string());
    }
}

impl ExtensionManager {
    pub async fn handle_search(
        &self,
        extension_name: &str,
        query: String,
    ) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let extensions = self.extensions.read().await;

        if let Some(extension) = extensions.get(extension_name) {
            let results = extension.on_search(&query).await?;
            // Send results through channel for UI updates
            let _ = self.sender.send(ExtensionMessage::SearchResults(
                extension_name.to_string(),
                results.clone(),
            ));

            Ok(results)
        } else {
            Err(ExtensionError::NotFound(format!(
                "Extension '{extension_name}' not found"
            )))
        }
    }

    pub async fn handle_action(
        &self,
        extension_name: &str,
        action: String,
        item_id: Option<String>,
    ) -> Result<(), ExtensionError> {
        let extensions = self.extensions.read().await;

        if let Some(extension) = extensions.get(extension_name) {
            extension.on_action(&action, item_id.as_deref()).await?;
            let _ = self
                .sender
                .send(ExtensionMessage::ActionComplete(extension_name.to_string()));
            Ok(())
        } else {
            Err(ExtensionError::NotFound(format!(
                "Extension '{extension_name}' not found"
            )))
        }
    }

    pub fn get_sender(&self) -> Sender<ExtensionMessage> {
        self.sender.clone()
    }

    pub fn get_receiver(&self) -> &Receiver<ExtensionMessage> {
        &self.receiver
    }

    pub async fn get_extensions(&self) -> Vec<String> {
        self.extensions.read().await.keys().cloned().collect()
    }

    pub async fn get_extension(&self, name: &str) -> Option<Arc<dyn Extension>> {
        self.extensions.read().await.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtensionManager, find_command_entry_point, parse_package_commands};
    use crate::extension_trait::ExtensionError;

    /// Searching via ExtensionManager for an extension that was never loaded
    /// returns `ExtensionError::NotFound`.
    #[tokio::test]
    async fn handle_search_unknown_extension_returns_not_found() {
        let manager = ExtensionManager::new();
        let result = manager
            .handle_search("does-not-exist", "query".to_string())
            .await;
        assert!(
            matches!(result, Err(ExtensionError::NotFound(_))),
            "unknown extension should return NotFound, got: {result:?}",
        );
    }

    // ── parse_package_commands ──────────────────────────────────────────────

    #[test]
    fn parse_commands_returns_all_entries() {
        let json = r#"{
            "name": "my-ext",
            "commands": [
                {"name": "search", "title": "Search Items", "description": "Find stuff"},
                {"name": "quick-add", "title": "Quick Add"}
            ]
        }"#;
        let cmds = parse_package_commands(json);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "search");
        assert_eq!(cmds[0].title, "Search Items");
        assert_eq!(cmds[0].description.as_deref(), Some("Find stuff"));
        assert_eq!(cmds[1].name, "quick-add");
        assert_eq!(cmds[1].title, "Quick Add");
        assert!(cmds[1].description.is_none());
    }

    #[test]
    fn parse_commands_missing_title_falls_back_to_name() {
        let json = r#"{"commands": [{"name": "run"}]}"#;
        let cmds = parse_package_commands(json);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].title, "run");
    }

    #[test]
    fn parse_commands_no_commands_field_returns_empty() {
        let json = r#"{"name": "no-commands"}"#;
        assert!(parse_package_commands(json).is_empty());
    }

    #[test]
    fn parse_commands_invalid_json_returns_empty() {
        assert!(parse_package_commands("not json").is_empty());
    }

    #[test]
    fn parse_commands_skips_entries_missing_name() {
        let json = r#"{"commands": [{"title": "No Name"}, {"name": "valid", "title": "Valid"}]}"#;
        let cmds = parse_package_commands(json);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "valid");
    }

    // ── find_command_entry_point ────────────────────────────────────────────

    #[test]
    fn find_entry_point_finds_tsx_in_src() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let expected = src.join("search.tsx");
        std::fs::write(&expected, "// stub").unwrap();

        let found = find_command_entry_point(dir.path(), "search");
        assert_eq!(found.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn find_entry_point_finds_index_tsx_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cmd_dir = dir.path().join("src").join("run");
        std::fs::create_dir_all(&cmd_dir).unwrap();
        let expected = cmd_dir.join("index.tsx");
        std::fs::write(&expected, "// stub").unwrap();

        let found = find_command_entry_point(dir.path(), "run");
        assert_eq!(found.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn find_entry_point_returns_none_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_command_entry_point(dir.path(), "missing").is_none());
    }

    #[test]
    fn find_entry_point_prefers_tsx_over_index() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let cmd_dir = src.join("cmd");
        std::fs::create_dir_all(&cmd_dir).unwrap();
        let direct = src.join("cmd.tsx");
        let index = cmd_dir.join("index.tsx");
        std::fs::write(&direct, "// direct").unwrap();
        std::fs::write(&index, "// index").unwrap();

        // direct file (cmd.tsx) should win over cmd/index.tsx
        let found = find_command_entry_point(dir.path(), "cmd");
        assert_eq!(found.as_deref(), Some(direct.as_path()));
    }
}

// ── Package-style multi-command extensions ────────────────────────────────────

/// One command entry parsed from a Raycast-style `package.json`.
#[derive(Debug, Clone)]
pub struct PackageCommand {
    /// kebab-case identifier — maps to the entry-point filename and the mode key suffix.
    pub name: String,
    /// Human-readable display title shown in the launcher.
    pub title: String,
    /// Optional description for this command.
    pub description: Option<String>,
}

/// Parse the `commands` array from a Raycast-style `package.json` body.
/// Returns an empty vec when the field is absent or invalid — not an error.
pub fn parse_package_commands(json: &str) -> Vec<PackageCommand> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    let Some(arr) = v.get("commands").and_then(|c| c.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|cmd| {
            let name = cmd.get("name")?.as_str()?.to_string();
            let title = cmd
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or(&name)
                .to_string();
            let description = cmd
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            Some(PackageCommand {
                name,
                title,
                description,
            })
        })
        .collect()
}

/// Find the JS/TS entry-point file for a Raycast command inside `<package_dir>/src/`.
///
/// Tries in order:
///   `src/<name>.tsx`, `src/<name>.ts`, `src/<name>.js`,
///   `src/<name>/index.tsx`, `src/<name>/index.ts`, `src/<name>/index.js`
pub fn find_command_entry_point(package_dir: &Path, command_name: &str) -> Option<PathBuf> {
    let src = package_dir.join("src");
    let candidates = [
        src.join(format!("{command_name}.tsx")),
        src.join(format!("{command_name}.ts")),
        src.join(format!("{command_name}.js")),
        src.join(command_name).join("index.tsx"),
        src.join(command_name).join("index.ts"),
        src.join(command_name).join("index.js"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

impl ExtensionManager {
    /// Load all commands from a Raycast-style extension directory that contains
    /// a `package.json` with a `commands` array.
    ///
    /// Each command is registered under the key `<package_name>/<command_name>`.
    /// Returns the number of commands successfully loaded.
    pub async fn load_package_dir(
        &self,
        dir: PathBuf,
        package_name: &str,
    ) -> Result<usize, ExtensionError> {
        let pkg_path = dir.join("package.json");
        let json = fs::read_to_string(&pkg_path)
            .map_err(|e| ExtensionError::LoadError(format!("Failed to read package.json: {e}")))?;

        // Parse top-level fields for shared metadata
        let pkg_val: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| ExtensionError::LoadError(format!("Invalid package.json: {e}")))?;
        let pkg_version = pkg_val
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();
        let pkg_author = pkg_val
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let pkg_permissions: Vec<String> = pkg_val
            .get("permissions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let commands = parse_package_commands(&json);
        if commands.is_empty() {
            return Err(ExtensionError::LoadError(format!(
                "package.json in {dir:?} has no commands"
            )));
        }

        let mut loaded = 0;
        for cmd in &commands {
            let Some(entry_path) = find_command_entry_point(&dir, &cmd.name) else {
                eprintln!("No entry point found for command '{}' in {dir:?}", cmd.name);
                continue;
            };

            let ext_key = format!("{package_name}/{}", cmd.name);
            let language = match entry_path.extension().and_then(|s| s.to_str()) {
                Some("ts" | "tsx") => ExtensionLanguage::TypeScript,
                _ => ExtensionLanguage::JavaScript,
            };

            let user_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".pterry")
                .join("extensions");
            let metadata = ExtensionMetadata {
                name: ext_key.clone(),
                title: Some(cmd.title.clone()),
                version: pkg_version.clone(),
                description: cmd.description.clone(),
                author: pkg_author.clone(),
                language,
                entry_point: entry_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                permissions: pkg_permissions.clone(),
                auto_load: false,
                preferences: vec![],
                is_development: !dir.starts_with(&user_dir),
            };

            let raw_code = match fs::read_to_string(&entry_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to read {entry_path:?}: {e}");
                    continue;
                }
            };

            let file_path_str = entry_path.to_str().unwrap_or("unknown");
            let is_raycast = crate::transpiler::is_raycast_api_extension(&raw_code);
            let (js_code, raycast_cjs_mode) = if is_raycast {
                match crate::transpiler::transpile_for_raycast(&raw_code, file_path_str) {
                    Ok(cjs) => (cjs, true),
                    Err(e) => {
                        eprintln!("Transpile error for '{}': {e}", cmd.name);
                        continue;
                    }
                }
            } else {
                match crate::transpiler::transpile_typescript(&raw_code, file_path_str) {
                    Ok(js) => (js, false),
                    Err(e) => {
                        eprintln!("Transpile error for '{}': {e}", cmd.name);
                        continue;
                    }
                }
            };

            let extension = match crate::js_extension::JsExtension::new_with_sender_and_clipboard(
                metadata,
                &js_code,
                raycast_cjs_mode,
                self.sender.clone(),
                self.clipboard_manager.clone(),
            )
            .await
            {
                Ok(ext) => ext,
                Err(e) => {
                    eprintln!("Failed to load command '{}': {e}", cmd.name);
                    continue;
                }
            };

            self.extensions
                .write()
                .await
                .insert(ext_key.clone(), Arc::new(extension));
            let _ = self.sender.send(ExtensionMessage::ExtensionLoaded(ext_key));
            loaded += 1;
        }

        Ok(loaded)
    }
}

impl ExtensionManager {
    pub async fn broadcast_search(&self, query: String) -> HashMap<String, Vec<ExtensionItem>> {
        let extensions = self.extensions.read().await;
        let mut all_results = HashMap::new();

        for (name, extension) in extensions.iter() {
            let auto = extension.metadata().auto_load;
            if !auto {
                // Non-auto-load: surface launcher_item() so users can discover
                // and enter the extension's dedicated mode.
                if let Some(item) = extension.launcher_item() {
                    let q = query.to_lowercase();
                    if q.is_empty()
                        || item.title.to_lowercase().contains(&q)
                        || item
                            .subtitle
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&q)
                    {
                        all_results.insert(name.clone(), vec![item]);
                    }
                }
                continue;
            }
            match extension.on_search(&query).await {
                Ok(results) => {
                    if !results.is_empty() {
                        all_results.insert(name.clone(), results);
                    }
                }
                Err(e) => {
                    eprintln!("Extension '{name}' search error: {e}");
                }
            }
        }

        all_results
    }
}
