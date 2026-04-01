use crate::extension_manager::ExtensionMessage;
use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata,
};
use crate::modes;
use crate::store_state::StoreState;
use async_trait::async_trait;
use crossbeam_channel::Sender;
use serde_json::Value;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const EXTENSION_LIST_URL: &str =
    "https://raw.githubusercontent.com/raycast/extensions/main/.github/extensionName2Folder.json";

const CACHE_TTL: Duration = Duration::from_secs(300);

struct Cache {
    /// Sorted vec of (extension_title, folder_name).
    entries: Vec<(String, String)>,
    fetched_at: Instant,
}

pub struct StoreRaycastExtension {
    metadata: ExtensionMetadata,
    cache: Arc<RwLock<Option<Cache>>>,
    list_url: String,
    sender: Option<Sender<ExtensionMessage>>,
}

impl fmt::Debug for StoreRaycastExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreRaycastExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl StoreRaycastExtension {
    pub fn new() -> Self {
        Self::with_url(EXTENSION_LIST_URL.to_string(), None)
    }

    pub fn new_with_sender(sender: Sender<ExtensionMessage>) -> Self {
        Self::with_url(EXTENSION_LIST_URL.to_string(), Some(sender))
    }

    fn with_url(list_url: String, sender: Option<Sender<ExtensionMessage>>) -> Self {
        let metadata = ExtensionMetadata {
            name: modes::STORE_RAYCAST.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Browse Raycast extensions".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "store_raycast_extension.rs".to_string(),
            permissions: vec!["network".to_string()],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self {
            metadata,
            cache: Arc::new(RwLock::new(None)),
            list_url,
            sender,
        }
    }

    async fn get_entries(&self) -> Vec<(String, String)> {
        {
            let guard = self.cache.read().await;
            if let Some(ref c) = *guard
                && c.fetched_at.elapsed() < CACHE_TTL
            {
                return c.entries.clone();
            }
        }

        let url = self.list_url.clone();
        let entries = tokio::task::spawn_blocking(move || {
            ureq::get(&url)
                .call()
                .ok()
                .and_then(|resp| resp.into_string().ok())
                .and_then(|body| {
                    serde_json::from_str::<std::collections::HashMap<String, String>>(&body).ok()
                })
                .map(|map| {
                    let mut v: Vec<(String, String)> = map.into_iter().collect();
                    v.sort_by(|a, b| a.0.cmp(&b.0));
                    v
                })
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        *self.cache.write().await = Some(Cache {
            entries: entries.clone(),
            fetched_at: Instant::now(),
        });
        entries
    }

    fn send_toast(&self, style: &str, title: &str, message: &str) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(ExtensionMessage::ShowToast(
                style.to_string(),
                title.to_string(),
                message.to_string(),
            ));
        }
    }
}

impl Default for StoreRaycastExtension {
    fn default() -> Self {
        Self::new()
    }
}

/// Filter Raycast extension entries (title, folder) by query (case-insensitive).
/// Extracted as a pure function for testability.
pub fn filter_raycast_items(entries: &[(String, String)], query: &str) -> Vec<ExtensionItem> {
    let q = query.to_lowercase();
    entries
        .iter()
        .filter(|(title, folder)| {
            q.is_empty()
                || title.to_lowercase().contains(&q)
                || folder.to_lowercase().contains(&q)
        })
        .map(|(title, folder)| ExtensionItem {
            title: title.clone(),
            subtitle: Some(folder.clone()),
            icon: Some("⚡".to_string()),
            action: format!("store-raycast:install:{folder}"),
            id: Some(folder.clone()),
            detail: Some(format!(
                "## {title}\n\n**Folder:** `{folder}`\n\nFrom the official Raycast extension repository."
            )),
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
        .collect()
}

/// Recursively download a GitHub directory via the GitHub Contents API.
/// `api_url` is the contents API URL for the directory.
/// `dest` is the local directory to write files into.
/// `token` is an optional GitHub personal-access token for authenticated requests.
fn download_github_dir(api_url: &str, dest: &Path, token: Option<&str>) -> Result<(), String> {
    let mut req = ureq::get(api_url)
        .set("User-Agent", "pterry/0.1")
        .set("Accept", "application/vnd.github.v3+json");
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }

    let body = req
        .call()
        .map_err(|e| format!("GitHub API error for {api_url}: {e}"))?
        .into_string()
        .map_err(|e| format!("read GitHub response: {e}"))?;

    let items: Vec<Value> =
        serde_json::from_str(&body).map_err(|e| format!("parse GitHub response: {e}"))?;

    std::fs::create_dir_all(dest).map_err(|e| format!("create dir {}: {e}", dest.display()))?;

    for item in &items {
        let name = item["name"].as_str().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let item_type = item["type"].as_str().unwrap_or("file");

        match item_type {
            "file" => {
                let download_url = match item["download_url"].as_str() {
                    Some(u) => u,
                    None => continue, // skip items without a download URL (e.g. submodules)
                };
                let mut dl_req = ureq::get(download_url).set("User-Agent", "pterry/0.1");
                if let Some(t) = token {
                    dl_req = dl_req.set("Authorization", &format!("Bearer {t}"));
                }
                let mut file_bytes = Vec::new();
                dl_req
                    .call()
                    .map_err(|e| format!("download {name}: {e}"))?
                    .into_reader()
                    .read_to_end(&mut file_bytes)
                    .map_err(|e| format!("read {name}: {e}"))?;
                std::fs::write(dest.join(name), file_bytes)
                    .map_err(|e| format!("write {name}: {e}"))?;
            }
            "dir" => {
                let subdir_url = match item["url"].as_str() {
                    Some(u) => u,
                    None => continue,
                };
                download_github_dir(subdir_url, &dest.join(name), token)?;
            }
            _ => {} // ignore symlinks, submodules, etc.
        }
    }

    Ok(())
}

/// Resolve the JS/TS entry point for an extension in `ext_dir`.
/// Checks `main` in `package.json`, then falls back to common file names.
fn find_entry_point(ext_dir: &Path) -> Result<PathBuf, String> {
    let pkg_json = ext_dir.join("package.json");
    if pkg_json.exists()
        && let Ok(content) = std::fs::read_to_string(&pkg_json)
        && let Ok(pkg) = serde_json::from_str::<Value>(&content)
        && let Some(main) = pkg["main"].as_str()
    {
        let entry = ext_dir.join(main);
        if entry.exists() {
            return Ok(entry);
        }
    }

    for candidate in &[
        "src/index.tsx",
        "src/index.ts",
        "src/index.js",
        "index.tsx",
        "index.ts",
        "index.js",
    ] {
        let p = ext_dir.join(candidate);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(format!("no entry point found in {}", ext_dir.display()))
}

#[async_trait]
impl Extension for StoreRaycastExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let entries = self.get_entries().await;
        Ok(filter_raycast_items(&entries, query))
    }

    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError> {
        if !action.starts_with("install:") {
            return Ok(());
        }
        let folder = match item_id {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => return Ok(()),
        };

        self.send_toast(
            "animated",
            "Installing…",
            &format!("Downloading '{folder}' from Raycast store"),
        );

        let folder_clone = folder.clone();
        let token = crate::settings::Settings::load().github_token;

        // Step 1 + 2: Download sources and fetch npm deps in a blocking thread.
        let result =
            tokio::task::spawn_blocking(move || -> Result<(tempfile::TempDir, PathBuf), String> {
                let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
                let ext_dir = tmp.path().to_path_buf();

                let api_url = format!(
                    "https://api.github.com/repos/raycast/extensions/contents/extensions/{folder_clone}"
                );
                download_github_dir(&api_url, &ext_dir, token.as_deref())?;

                let pkg_json = ext_dir.join("package.json");
                if pkg_json.exists() {
                    let node_modules = ext_dir.join("node_modules");
                    crate::npm_fetcher::fetch_all_deps(&pkg_json, &node_modules)
                        .map_err(|e| format!("npm deps: {e}"))?;
                }

                let entry = find_entry_point(&ext_dir)?;
                Ok((tmp, entry))
            })
            .await
            .unwrap_or_else(|e| Err(format!("task panicked: {e}")));

        let (tmp_dir, entry_path) = match result {
            Ok(pair) => pair,
            Err(e) => {
                self.send_toast("failure", "Install Failed", &e);
                return Ok(());
            }
        };

        // Step 3: Bundle with rolldown (async).
        let cwd = tmp_dir.path().to_path_buf();
        let bundle_result = crate::bundler::bundle_extension(&entry_path, &cwd).await;
        drop(tmp_dir); // clean up temp dir after bundling

        let bundled_js = match bundle_result {
            Ok(js) => js,
            Err(e) => {
                self.send_toast("failure", "Install Failed", &format!("Bundle error: {e}"));
                return Ok(());
            }
        };

        // Step 4: Write bundled file to ~/.pterry/extensions/<folder>/index.js
        let dest_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pterry")
            .join("extensions")
            .join(&folder);

        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            self.send_toast("failure", "Install Failed", &format!("Create dir: {e}"));
            return Ok(());
        }
        if let Err(e) = std::fs::write(dest_dir.join("index.js"), &bundled_js) {
            self.send_toast("failure", "Install Failed", &format!("Write file: {e}"));
            return Ok(());
        }

        // Step 5: Record install state.
        let mut state = StoreState::load();
        state.mark_installed(&folder, "unknown", "raycast");
        let _ = state.save();

        // Step 6: Notify the user.
        self.send_toast(
            "success",
            "Extension Installed",
            &format!("'{folder}' installed — restart to load it"),
        );

        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entries(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(t, f)| (t.to_string(), f.to_string()))
            .collect()
    }

    #[test]
    fn empty_query_returns_all() {
        let entries = make_entries(&[("GitHub", "github"), ("Jira", "jira")]);
        let items = filter_raycast_items(&entries, "");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn query_filters_by_title() {
        let entries = make_entries(&[("GitHub", "github"), ("Jira", "jira")]);
        let items = filter_raycast_items(&entries, "github");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("github"));
    }

    #[test]
    fn query_filters_by_folder() {
        let entries = make_entries(&[("Git Extension", "git-ext"), ("Slack", "slack")]);
        let items = filter_raycast_items(&entries, "git-ext");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("git-ext"));
    }

    #[test]
    fn query_is_case_insensitive() {
        let entries = make_entries(&[("GitHub", "github")]);
        let upper = filter_raycast_items(&entries, "GITHUB");
        let lower = filter_raycast_items(&entries, "github");
        assert_eq!(upper.len(), lower.len());
        assert_eq!(upper.len(), 1);
    }

    #[test]
    fn unmatched_query_returns_empty() {
        let entries = make_entries(&[("GitHub", "github")]);
        let items = filter_raycast_items(&entries, "zzznomatch");
        assert!(items.is_empty());
    }

    #[test]
    fn item_action_routes_to_store_raycast_extension() {
        let entries = make_entries(&[("My Extension", "my-extension")]);
        let items = filter_raycast_items(&entries, "");
        assert_eq!(items[0].action, "store-raycast:install:my-extension");
    }

    #[test]
    fn item_has_detail_with_title_and_folder() {
        let entries = make_entries(&[("My Extension", "my-extension")]);
        let items = filter_raycast_items(&entries, "");
        let detail = items[0].detail.as_deref().unwrap_or("");
        assert!(detail.contains("My Extension"));
        assert!(detail.contains("my-extension"));
    }

    #[test]
    fn store_raycast_not_auto_loaded() {
        let ext = StoreRaycastExtension::new();
        assert!(!ext.metadata().auto_load);
    }

    #[test]
    fn store_raycast_has_correct_name() {
        let ext = StoreRaycastExtension::new();
        assert_eq!(ext.metadata().name, crate::modes::STORE_RAYCAST);
    }

    #[test]
    fn find_entry_point_prefers_package_json_main() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.ts"), "").unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"main": "src/main.ts"}"#,
        )
        .unwrap();
        let entry = find_entry_point(dir.path()).unwrap();
        assert_eq!(entry, src.join("main.ts"));
    }

    #[test]
    fn find_entry_point_falls_back_to_src_index_tsx() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("index.tsx"), "").unwrap();
        let entry = find_entry_point(dir.path()).unwrap();
        assert_eq!(entry, src.join("index.tsx"));
    }

    #[test]
    fn find_entry_point_falls_back_to_index_js() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("index.js"), "").unwrap();
        let entry = find_entry_point(dir.path()).unwrap();
        assert_eq!(entry, dir.path().join("index.js"));
    }

    #[test]
    fn find_entry_point_error_when_nothing_found() {
        let dir = tempdir().unwrap();
        let result = find_entry_point(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no entry point found"));
    }

    #[test]
    fn new_with_sender_has_sender() {
        let (sender, _recv) = crossbeam_channel::unbounded();
        let ext = StoreRaycastExtension::new_with_sender(sender);
        assert!(ext.sender.is_some());
    }
}
