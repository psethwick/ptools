use rolldown::{BundlerOptions, InputItem, IsExternal, OutputFormat};
use std::path::Path;

/// Bundle a JS/TS extension from the given entry point using rolldown.
///
/// `cwd` is the extension directory; `node_modules` inside it will be resolved
/// automatically by rolldown's module resolver.
///
/// `@raycast/api` and `@raycast/utils` are marked as external because our
/// QuickJS runtime injects them as virtual modules at execution time.
///
/// Returns the bundled CommonJS JavaScript source as a string.
pub async fn bundle_extension(entry: &Path, cwd: &Path) -> Result<String, String> {
    let entry_str = entry.to_string_lossy().into_owned();
    let cwd_path = cwd.to_path_buf();

    let options = BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("index".to_string()),
            import: entry_str,
        }]),
        cwd: Some(cwd_path),
        format: Some(OutputFormat::Cjs),
        // Mark raycast packages as external — provided by our runtime shim.
        external: Some(IsExternal::from(vec![
            "@raycast/api".to_string(),
            "@raycast/utils".to_string(),
        ])),
        ..Default::default()
    };

    let mut bundler =
        rolldown::Bundler::new(options).map_err(|e| format!("bundler init: {e:?}"))?;

    let output = bundler
        .generate()
        .await
        .map_err(|e| format!("bundle error: {e:?}"))?;

    // Collect code from all output chunks (typically just one for single-entry bundles).
    let js: String = output
        .assets
        .iter()
        .map(|asset| {
            std::str::from_utf8(asset.content_as_bytes())
                .unwrap_or("")
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");

    if js.trim().is_empty() {
        return Err("bundler produced no output".to_string());
    }

    Ok(js)
}
