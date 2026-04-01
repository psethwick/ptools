use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Parse the `dependencies` map from a `package.json` file.
/// Returns an empty map if the file has no `dependencies` key.
/// Returns an error if the file cannot be read or is not valid JSON.
pub fn parse_dependencies(package_json_path: &Path) -> Result<HashMap<String, String>, String> {
    let content = std::fs::read_to_string(package_json_path)
        .map_err(|e| format!("read package.json: {e}"))?;
    let pkg: Value =
        serde_json::from_str(&content).map_err(|e| format!("parse package.json: {e}"))?;

    let deps = match pkg.get("dependencies").and_then(|d| d.as_object()) {
        Some(d) => d
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("latest").to_string()))
            .collect(),
        None => HashMap::new(),
    };
    Ok(deps)
}

/// Download and extract a single npm package (latest version) into `node_modules/<name>`.
/// Skips silently if the target directory already exists.
/// Uses `registry.npmjs.org` to resolve the tarball URL.
pub fn fetch_package(name: &str, node_modules: &Path) -> Result<(), String> {
    let target = node_modules.join(name);
    if target.exists() {
        return Ok(());
    }

    // Fetch registry metadata for the latest version.
    let registry_url = format!("https://registry.npmjs.org/{name}/latest");
    let body = ureq::get(&registry_url)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("registry fetch {name}: {e}"))?
        .into_string()
        .map_err(|e| format!("read registry {name}: {e}"))?;

    let meta: Value =
        serde_json::from_str(&body).map_err(|e| format!("parse registry JSON {name}: {e}"))?;

    let tarball_url = meta["dist"]["tarball"]
        .as_str()
        .ok_or_else(|| format!("no tarball URL for {name}"))?
        .to_string();

    // Download tarball bytes.
    let mut bytes = Vec::new();
    ureq::get(&tarball_url)
        .call()
        .map_err(|e| format!("download tarball {name}: {e}"))?
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read tarball {name}: {e}"))?;

    // Extract .tgz: npm tarballs use a "package/" prefix inside the archive.
    let cursor = std::io::Cursor::new(bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);

    std::fs::create_dir_all(&target).map_err(|e| format!("create dir: {e}"))?;

    for entry in archive
        .entries()
        .map_err(|e| format!("tar entries {name}: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("tar entry {name}: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("entry path {name}: {e}"))?
            .into_owned();

        // Strip the leading "package/" component npm tarballs include.
        let rel = match path.strip_prefix("package") {
            Ok(r) => r.to_path_buf(),
            Err(_) => path,
        };

        if rel.as_os_str().is_empty() {
            continue;
        }

        let dest = target.join(&rel);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| format!("create subdir: {e}"))?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {e}"))?;
            }
            let mut file = std::fs::File::create(&dest).map_err(|e| format!("create file: {e}"))?;
            std::io::copy(&mut entry, &mut file).map_err(|e| format!("write file: {e}"))?;
        }
    }

    Ok(())
}

/// Fetch all direct `dependencies` listed in `package.json` into `node_modules`.
/// Does nothing if the package has no dependencies.
pub fn fetch_all_deps(package_json: &Path, node_modules: &Path) -> Result<(), String> {
    let deps = parse_dependencies(package_json)?;
    if deps.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(node_modules).map_err(|e| format!("create node_modules: {e}"))?;
    for name in deps.keys() {
        fetch_package(name, node_modules)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_package_json(dir: &Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("package.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_dependencies_empty_object_returns_empty() {
        let dir = tempdir().unwrap();
        let path = write_package_json(dir.path(), "{}");
        let deps = parse_dependencies(&path).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_dependencies_no_deps_key_returns_empty() {
        let dir = tempdir().unwrap();
        let path = write_package_json(dir.path(), r#"{"name": "test", "version": "1.0.0"}"#);
        let deps = parse_dependencies(&path).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_dependencies_returns_all_entries() {
        let dir = tempdir().unwrap();
        let path = write_package_json(
            dir.path(),
            r#"{"dependencies": {"react": "^18.0.0", "lodash": "^4.17.0"}}"#,
        );
        let deps = parse_dependencies(&path).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps["react"], "^18.0.0");
        assert_eq!(deps["lodash"], "^4.17.0");
    }

    #[test]
    fn parse_dependencies_missing_file_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = parse_dependencies(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("read package.json"));
    }

    #[test]
    fn parse_dependencies_invalid_json_returns_error() {
        let dir = tempdir().unwrap();
        let path = write_package_json(dir.path(), "not valid json {{{}");
        let result = parse_dependencies(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parse package.json"));
    }

    #[test]
    fn fetch_all_deps_no_deps_succeeds_without_network() {
        let dir = tempdir().unwrap();
        let pkg_json = write_package_json(dir.path(), "{}");
        let node_modules = dir.path().join("node_modules");
        // No network required for empty deps.
        let result = fetch_all_deps(&pkg_json, &node_modules);
        assert!(result.is_ok());
        // node_modules is not created when there are no deps.
        assert!(!node_modules.exists());
    }

    #[test]
    fn fetch_all_deps_dev_deps_ignored() {
        let dir = tempdir().unwrap();
        // devDependencies should NOT be fetched — only dependencies key matters.
        let path = write_package_json(
            dir.path(),
            r#"{"devDependencies": {"typescript": "^5.0.0"}}"#,
        );
        let node_modules = dir.path().join("node_modules");
        let result = fetch_all_deps(&path, &node_modules);
        assert!(result.is_ok());
        assert!(!node_modules.exists());
    }
}
