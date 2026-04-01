use crate::clipboard_manager::{ClipboardContent, ClipboardManager};
use crate::extension_manager::ExtensionMessage;
use crate::extension_trait::{Extension, ExtensionError, ExtensionItem, ExtensionMetadata};
use async_trait::async_trait;
use crossbeam_channel::Sender;
use rquickjs::prelude::Rest;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Embedded @raycast/api shim (injected before Raycast-style CJS extensions).
const RAYCAST_SHIM: &str = include_str!("raycast_shim.js");

/// Persistent QuickJS runtime state for one extension.
///
/// `Context` is listed before `Runtime` so Rust drops it first (fields drop
/// in declaration order), satisfying the QuickJS invariant that a context must
/// be freed before its runtime.
struct RuntimeCtx {
    context: rquickjs::Context,
    runtime: rquickjs::Runtime,
}

// SAFETY: QuickJS (`Rc`-based) is not thread-safe on its own, but `RuntimeCtx`
// is always accessed exclusively through a `Mutex<RuntimeCtx>`, ensuring only
// one thread ever enters QuickJS at a time. No references cross thread boundaries.
unsafe impl Send for RuntimeCtx {}
unsafe impl Sync for RuntimeCtx {}

/// Maximum time a JS extension is allowed to run `on_search` or `on_action`
/// before the call is considered hung and an error is returned.
const JS_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Compute the per-extension support directory: `~/.pterry/extension-data/<name>/`.
fn extension_support_dir(ext_name: &str) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".pterry")
        .join("extension-data")
        .join(ext_name)
}

/// Compute the per-extension assets directory: `~/.pterry/extensions/<name>/assets`.
///
/// Matches the convention for both single-file extensions (`my-ext.js` installed next to a
/// sibling `my-ext/assets/` directory) and directory extensions (`my-ext/assets/`).
fn extension_assets_dir(ext_name: &str) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".pterry")
        .join("extensions")
        .join(ext_name)
        .join("assets")
}

/// Read the persistent key-value store from `<support_dir>/store.json`.
/// Returns an empty map on missing file or parse error.
fn read_store(support_dir: &std::path::Path) -> std::collections::HashMap<String, String> {
    let path = support_dir.join("store.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the key-value store to `<support_dir>/store.json`, creating dirs as needed.
fn write_store(support_dir: &std::path::Path, map: &std::collections::HashMap<String, String>) {
    let path = support_dir.join("store.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(map) {
        let _ = std::fs::write(path, json);
    }
}

pub struct JsExtension {
    metadata: ExtensionMetadata,
    /// Shared results store written by `raycast.updateList()` and read after each search.
    results: Arc<Mutex<Vec<ExtensionItem>>>,
    /// One QuickJS runtime + context, reused across every `on_search` / `on_action` call.
    /// Wrapped in `Arc` so it can be moved into `spawn_blocking` closures.
    runtime_ctx: Arc<Mutex<RuntimeCtx>>,
}

impl fmt::Debug for JsExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl JsExtension {
    /// Create a new JS extension without a UI sender (used by tests and simple callers).
    ///
    /// `raycast_cjs_mode` — when `true`, the code has been CJS-transformed by
    /// `transpile_for_raycast` and uses `@raycast/api` imports. The shim is
    /// injected, `require`/`exports`/`module` globals are set up, and the
    /// default export is automatically bootstrapped as a React component.
    pub async fn new(
        metadata: ExtensionMetadata,
        js_code: &str,
        raycast_cjs_mode: bool,
    ) -> Result<Self, ExtensionError> {
        Self::new_inner(metadata, js_code, raycast_cjs_mode, None, None).await
    }

    /// Create a new JS extension with a sender for UI messages (used by the extension manager).
    ///
    /// The sender is used to forward `showToast` calls from JS to the UI layer.
    pub async fn new_with_sender(
        metadata: ExtensionMetadata,
        js_code: &str,
        raycast_cjs_mode: bool,
        sender: Sender<ExtensionMessage>,
    ) -> Result<Self, ExtensionError> {
        Self::new_inner(metadata, js_code, raycast_cjs_mode, Some(sender), None).await
    }

    /// Create a new JS extension with a sender and clipboard access.
    pub async fn new_with_sender_and_clipboard(
        metadata: ExtensionMetadata,
        js_code: &str,
        raycast_cjs_mode: bool,
        sender: Sender<ExtensionMessage>,
        clipboard_manager: Option<Arc<ClipboardManager>>,
    ) -> Result<Self, ExtensionError> {
        Self::new_inner(
            metadata,
            js_code,
            raycast_cjs_mode,
            Some(sender),
            clipboard_manager,
        )
        .await
    }

    async fn new_inner(
        metadata: ExtensionMetadata,
        js_code: &str,
        raycast_cjs_mode: bool,
        sender_opt: Option<Sender<ExtensionMessage>>,
        clipboard_opt: Option<Arc<ClipboardManager>>,
    ) -> Result<Self, ExtensionError> {
        use rquickjs::{CatchResultExt, Context, Runtime};

        let results: Arc<Mutex<Vec<ExtensionItem>>> = Arc::new(Mutex::new(Vec::new()));

        let runtime = Runtime::new().map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        let context =
            Context::full(&runtime).map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        let has_network = metadata.permissions.contains(&"network".to_string());
        let js_code_owned = js_code.to_string();
        let support_path = extension_support_dir(&metadata.name);
        let assets_path = extension_assets_dir(&metadata.name);
        // Create the support dir eagerly so extensions can use it immediately.
        let _ = std::fs::create_dir_all(&support_path);

        // Derive command_name from entry_point stem (e.g. "search.tsx" → "search").
        let command_name = std::path::Path::new(&metadata.entry_point)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&metadata.name)
            .to_string();
        let extension_name = metadata.name.clone();
        let is_development = metadata.is_development;

        context.with(|ctx| -> Result<(), ExtensionError> {
            Self::setup_console(ctx.clone()).map_err(|e| {
                ExtensionError::RuntimeError(format!("Failed to setup console: {e}"))
            })?;
            Self::setup_fetch(ctx.clone(), has_network)
                .map_err(|e| ExtensionError::RuntimeError(format!("Failed to setup fetch: {e}")))?;
            Self::setup_raycast_object(
                ctx.clone(),
                results.clone(),
                sender_opt.clone(),
                clipboard_opt.clone(),
                support_path.clone(),
                assets_path,
                extension_name,
                command_name,
                is_development,
            )?;

            if raycast_cjs_mode {
                Self::setup_raycast_cjs(ctx.clone(), &js_code_owned, &metadata.name)?;
            } else {
                // Legacy mode: eval and expect onSearch/onAction globals.
                ctx.eval::<(), _>(js_code_owned.as_str())
                    .catch(&ctx)
                    .map_err(|e| {
                        ExtensionError::RuntimeError(format!("JS initialisation error: {e}"))
                    })?;
            }

            Ok(())
        })?;

        Ok(JsExtension {
            metadata,
            results,
            runtime_ctx: Arc::new(Mutex::new(RuntimeCtx { context, runtime })),
        })
    }

    /// Build and install the `raycast` global object in the QuickJS context.
    ///
    /// Registers:
    /// - `raycast.updateList(items)` — stores search results produced by the extension
    /// - `raycast.showToast(style, title, message)` — only if `sender_opt` is `Some`
    /// - `raycast.clipboardRead()` / `raycast.clipboardWrite(text)` — only if `clipboard_opt` is `Some`
    /// - `raycast.supportPath()` — returns the extension's support directory path
    /// - `raycast.assetsPath()` — returns the extension's assets directory path
    /// - `raycast.storageGet/Set/Del/Clear/All` — persistent key-value store in `<supportPath>/store.json`
    #[allow(clippy::too_many_arguments)]
    fn setup_raycast_object(
        ctx: rquickjs::Ctx<'_>,
        results: Arc<Mutex<Vec<ExtensionItem>>>,
        sender_opt: Option<Sender<ExtensionMessage>>,
        clipboard_opt: Option<Arc<ClipboardManager>>,
        support_path: std::path::PathBuf,
        assets_path: std::path::PathBuf,
        extension_name: String,
        command_name: String,
        is_development: bool,
    ) -> Result<(), ExtensionError> {
        let raycast_obj = rquickjs::Object::new(ctx.clone())
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.updateList(items) ---
        // Used both by legacy extensions directly and by the @raycast/api shim's reconciler.
        let update_list_func = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                if let Some(array) = args.first().and_then(|v| v.clone().into_array()) {
                    let mut items = Vec::new();
                    for i in 0..array.len() {
                        if let Ok(obj) = array.get::<rquickjs::Object>(i)
                            && let (Ok(title), Ok(action)) = (
                                obj.get::<_, rquickjs::String>("title"),
                                obj.get::<_, rquickjs::String>("action"),
                            )
                        {
                            let subtitle = obj
                                .get::<_, rquickjs::String>("subtitle")
                                .ok()
                                .and_then(|s| s.to_string().ok());
                            let icon = obj
                                .get::<_, rquickjs::String>("icon")
                                .ok()
                                .and_then(|s| s.to_string().ok());
                            let id = obj
                                .get::<_, rquickjs::String>("id")
                                .ok()
                                .and_then(|s| s.to_string().ok());
                            let detail = obj
                                .get::<_, rquickjs::String>("detail")
                                .ok()
                                .and_then(|s| s.to_string().ok());
                            let accessories: Vec<String> = obj
                                .get::<_, rquickjs::Array>("accessories")
                                .ok()
                                .map(|arr| {
                                    (0..arr.len())
                                        .filter_map(|k| arr.get::<rquickjs::Value>(k).ok())
                                        .filter_map(|v| v.into_object())
                                        .filter_map(|acc| {
                                            // Prefer .text, fall back to .tag.value
                                            if let Ok(s) = acc.get::<_, rquickjs::String>("text")
                                                && let Ok(t) = s.to_string()
                                            {
                                                return Some(t);
                                            }
                                            if let Ok(tag) = acc.get::<_, rquickjs::Object>("tag")
                                                && let Ok(s) =
                                                    tag.get::<_, rquickjs::String>("value")
                                                && let Ok(t) = s.to_string()
                                            {
                                                return Some(t);
                                            }
                                            None
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            let extra_actions: Vec<crate::extension_trait::ExtraAction> = obj
                                .get::<_, rquickjs::Array>("extraActions")
                                .ok()
                                .map(|arr| {
                                    (0..arr.len())
                                        .filter_map(|k| arr.get::<rquickjs::Object>(k).ok())
                                        .filter_map(|ea| {
                                            let title = ea
                                                .get::<_, rquickjs::String>("title")
                                                .ok()?
                                                .to_string()
                                                .ok()?;
                                            let action = ea
                                                .get::<_, rquickjs::String>("action")
                                                .ok()?
                                                .to_string()
                                                .ok()
                                                .unwrap_or_default();
                                            let icon = ea
                                                .get::<_, rquickjs::String>("icon")
                                                .ok()
                                                .and_then(|s| s.to_string().ok());
                                            let shortcut = ea
                                                .get::<_, rquickjs::String>("shortcut")
                                                .ok()
                                                .and_then(|s| s.to_string().ok());
                                            Some(crate::extension_trait::ExtraAction {
                                                title,
                                                action,
                                                icon,
                                                shortcut,
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            let grid_columns: Option<u8> = obj
                                .get::<_, rquickjs::Value>("gridColumns")
                                .ok()
                                .and_then(|v| v.as_int())
                                .and_then(|n| u8::try_from(n).ok())
                                .filter(|&c| c > 0);

                            items.push(ExtensionItem {
                                title: title.to_string().unwrap_or_default(),
                                subtitle,
                                icon,
                                action: action.to_string().unwrap_or_default(),
                                id,
                                detail,
                                accessories,
                                extra_actions,
                                detail_metadata: vec![],
                                thumbnail_rgba: None,
                                grid_columns,
                            });
                        }
                    }
                    if let Ok(mut guard) = results.lock() {
                        *guard = items;
                    }
                }
                Ok::<(), rquickjs::Error>(())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("updateList", update_list_func)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.showToast(style, title, message) / raycast.navigate(action) ---
        if let Some(sender) = sender_opt {
            // navigate(action) — called by the JS shim when push/pop navigation happens.
            let navigate_sender = sender.clone();
            let navigate_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let action = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let _ = navigate_sender.send(ExtensionMessage::Navigate(action));
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("navigate", navigate_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            let hide_sender = sender.clone();
            let show_toast_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let style = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let title = args
                        .get(1)
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let msg = args
                        .get(2)
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let _ = sender.send(ExtensionMessage::ShowToast(style, title, msg));
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("showToast", show_toast_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            // hideWindow() — extension explicitly requests the launcher to hide.
            let hide_window_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                    let _ = hide_sender.send(ExtensionMessage::HideWindow);
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("hideWindow", hide_window_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        }

        // --- raycast.clipboardRead() / raycast.clipboardWrite(text) ---
        if let Some(cm) = clipboard_opt {
            let cm_read = cm.clone();
            let clipboard_read_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                    let text = cm_read
                        .get_history()
                        .iter()
                        .find_map(|item| {
                            if let ClipboardContent::Text(t) = &item.content {
                                Some(t.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();
                    Ok::<String, rquickjs::Error>(text)
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("clipboardRead", clipboard_read_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            let cm_write = cm;
            let clipboard_write_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let text = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    if let Err(e) = cm_write.copy_to_clipboard(&text) {
                        eprintln!("[raycast] clipboardWrite error: {e}");
                    }
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("clipboardWrite", clipboard_write_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        }

        // --- raycast.open(url) ---
        // Opens a URL or file path with the system default handler.
        let open_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                let url = args
                    .first()
                    .and_then(|v| v.clone().into_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default();
                if !url.is_empty() {
                    crate::platform::open(&url);
                }
                Ok::<(), rquickjs::Error>(())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("open", open_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.homedir() / raycast.platform() ---
        let homedir_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                let home = dirs::home_dir()
                    .and_then(|p| p.to_str().map(str::to_string))
                    .unwrap_or_else(|| "/home/user".to_string());
                Ok::<String, rquickjs::Error>(home)
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("homedir", homedir_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        let platform_name = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else {
            "linux"
        };
        let platform_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(platform_name.to_string())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("platform", platform_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.supportPath() ---
        let sp_str = support_path.to_string_lossy().into_owned();
        let support_path_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(sp_str.clone())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("supportPath", support_path_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.assetsPath() ---
        let ap_str = assets_path.to_string_lossy().into_owned();
        let assets_path_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(ap_str.clone())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("assetsPath", assets_path_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.getPreferences() ---
        // Reads per-extension user preferences from `<support_path>/preferences.json`.
        // Returns the raw JSON string so the JS shim can JSON.parse it.
        // Returns `{}` when the file does not exist.
        {
            let prefs_path = support_path.join("preferences.json");
            let get_prefs_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                    let json =
                        std::fs::read_to_string(&prefs_path).unwrap_or_else(|_| "{}".to_string());
                    Ok::<String, rquickjs::Error>(json)
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("getPreferences", get_prefs_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        }

        // --- raycast.storageGet/Set/Del/Clear/All ---
        // Backed by <support_path>/store.json (a JSON object mapping string keys to string values).
        // Values are stored as JSON-serialised strings so the JS LocalStorage shim can
        // JSON.parse them back to their original types.
        {
            let sp = support_path.clone();
            let storage_get_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let key = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    Ok::<Option<String>, rquickjs::Error>(read_store(&sp).remove(&key))
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("storageGet", storage_get_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            let sp = support_path.clone();
            let storage_set_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let key = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let val = args
                        .get(1)
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let mut map = read_store(&sp);
                    map.insert(key, val);
                    write_store(&sp, &map);
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("storageSet", storage_set_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            let sp = support_path.clone();
            let storage_del_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                    let key = args
                        .first()
                        .and_then(|v| v.clone().into_string())
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let mut map = read_store(&sp);
                    map.remove(&key);
                    write_store(&sp, &map);
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("storageDel", storage_del_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            let sp = support_path.clone();
            let storage_clear_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                    write_store(&sp, &std::collections::HashMap::new());
                    Ok::<(), rquickjs::Error>(())
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("storageClear", storage_clear_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

            let sp = support_path;
            let storage_all_fn = rquickjs::Function::new(
                ctx.clone(),
                move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                    let map = read_store(&sp);
                    let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
                    Ok::<String, rquickjs::Error>(json)
                },
            )
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
            raycast_obj
                .set("storageAll", storage_all_fn)
                .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        }

        // --- raycast.extensionName() / raycast.commandName() / raycast.isDevelopment() ---
        // Expose the metadata fields the shim needs to populate `environment`.
        let ext_name_val = extension_name.clone();
        let ext_name_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(ext_name_val.clone())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("extensionName", ext_name_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        let cmd_name_val = command_name.clone();
        let cmd_name_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(cmd_name_val.clone())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("commandName", cmd_name_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        let is_dev_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<bool, rquickjs::Error>(is_development)
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("isDevelopment", is_dev_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.getTheme() ---
        // Returns "dark" or "light" by reading the theme field from settings.json.
        // "system" and missing values default to "dark".
        // Extensions use this to set environment.theme at load time.
        let get_theme_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                let theme = crate::settings::Settings::load()
                    .theme
                    .unwrap_or_default();
                let js_theme = if theme == "light" { "light" } else { "dark" };
                Ok::<String, rquickjs::Error>(js_theme.to_string())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("getTheme", get_theme_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.getSelectedText() ---
        // Returns the currently selected text from the OS as a plain string.
        // The JS shim wraps this in a Promise; the native binding is synchronous.
        let get_selected_text_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, _args: Rest<rquickjs::Value>| {
                Ok::<String, rquickjs::Error>(crate::platform::get_selected_text())
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("getSelectedText", get_selected_text_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        // --- raycast.cryptoHash(algorithm, data) ---
        // Implements the Node.js `crypto` module `createHash` primitive.
        // algorithm: "md5" | "sha1" | "sha256" | "sha512"
        // data: string to hash
        // Returns a lowercase hex string.
        let crypto_hash_fn = rquickjs::Function::new(
            ctx.clone(),
            move |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                let algorithm = args
                    .first()
                    .and_then(|v| v.as_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default();
                let data = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default();
                let hex = crypto_hash(&algorithm, &data);
                Ok::<String, rquickjs::Error>(hex)
            },
        )
        .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;
        raycast_obj
            .set("cryptoHash", crypto_hash_fn)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        ctx.globals()
            .set("raycast", raycast_obj)
            .map_err(|e| ExtensionError::RuntimeError(e.to_string()))?;

        Ok(())
    }

    /// Set up the Raycast CJS environment and evaluate a CJS-transformed extension.
    ///
    /// Steps:
    /// 1. Evaluate the embedded @raycast/api shim (sets up `require`, `babelHelpers`, etc.)
    /// 2. Set up `module`, `exports` globals for the CJS wrapper
    /// 3. Evaluate the CJS-transformed extension code
    /// 4. Extract `module.exports.default` and call `__raycastBootstrap(default)`
    fn setup_raycast_cjs(
        ctx: rquickjs::Ctx,
        js_code: &str,
        ext_name: &str,
    ) -> Result<(), ExtensionError> {
        use rquickjs::CatchResultExt;

        // 1. Inject the @raycast/api shim
        ctx.eval::<(), _>(RAYCAST_SHIM)
            .catch(&ctx)
            .map_err(|e| ExtensionError::RuntimeError(format!("Raycast shim error: {e}")))?;

        // 1b. Set the extension name so _extractAction can prefix action strings
        //     with "<ext-name>:" so they route to the correct extension in Rust.
        let set_name = format!("globalThis.__raycastExtensionName = {:?};", ext_name);
        ctx.eval::<(), _>(set_name.as_str())
            .catch(&ctx)
            .map_err(|e| {
                ExtensionError::RuntimeError(format!("Extension name setup error: {e}"))
            })?;

        // 2. Set up CJS module/exports globals
        ctx.eval::<(), _>(
            r#"
var __cjs_module = { exports: {} };
globalThis.module = __cjs_module;
globalThis.exports = __cjs_module.exports;
"#,
        )
        .catch(&ctx)
        .map_err(|e| ExtensionError::RuntimeError(format!("CJS globals setup error: {e}")))?;

        // 3. Evaluate the CJS-transformed extension code
        ctx.eval::<(), _>(js_code).catch(&ctx).map_err(|e| {
            ExtensionError::RuntimeError(format!("Raycast extension eval error: {e}"))
        })?;

        // 4. Bootstrap: if module.exports.default is a function, call __raycastBootstrap
        ctx.eval::<(), _>(
            r#"
(function() {
    var defaultExport = __cjs_module.exports && __cjs_module.exports.default;
    if (typeof defaultExport === 'function') {
        if (typeof globalThis.__raycastBootstrap === 'function') {
            globalThis.__raycastBootstrap(defaultExport);
        }
    }
})();
"#,
        )
        .catch(&ctx)
        .map_err(|e| ExtensionError::RuntimeError(format!("Raycast bootstrap error: {e}")))?;

        Ok(())
    }

    /// Setup console object for QuickJS compatibility.
    ///
    /// Registers `log`, `error`, `warn`, `debug`, and `info` — all five are
    /// needed because real extensions call `console.error()` / `console.warn()`
    /// etc. and QuickJS throws `TypeError: not a function` if they are absent.
    fn setup_console(ctx: rquickjs::Ctx) -> Result<(), rquickjs::Error> {
        let console_obj = rquickjs::Object::new(ctx.clone())?;

        /// Build a console method closure that prints `[JS <PREFIX>] <args>`.
        macro_rules! make_log_fn {
            ($prefix:literal) => {
                rquickjs::Function::new(
                    ctx.clone(),
                    |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                        let mut output = String::new();
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                output.push(' ');
                            }
                            match arg.clone().into_string() {
                                Some(s) => output.push_str(&s.to_string().unwrap_or_default()),
                                None => output.push_str(&format!("{arg:?}")),
                            }
                        }
                        println!(concat!("[JS ", $prefix, "] {}"), output);
                        Ok::<(), rquickjs::Error>(())
                    },
                )?
            };
        }

        console_obj.set("log", make_log_fn!("log"))?;
        console_obj.set("error", make_log_fn!("error"))?;
        console_obj.set("warn", make_log_fn!("warn"))?;
        console_obj.set("debug", make_log_fn!("debug"))?;
        console_obj.set("info", make_log_fn!("info"))?;
        ctx.globals().set("console", console_obj)?;
        Ok(())
    }

    /// Expose `fetch(url, opts?)` as a global in the QuickJS context.
    ///
    /// Requires the extension to declare `"network"` in its `permissions` list.
    /// Without it, a stub is installed that throws `PermissionError` at call time.
    ///
    /// When permitted, internally registers `_fetchRaw(url, method, body, headersJson)`
    /// as a native Rust function that performs a **blocking** HTTP request via
    /// `ureq`, then wraps it in a proper Promise-based `fetch` API via eval.
    ///
    /// The blocking call is acceptable here because extensions already run on
    /// a dedicated tokio task, and the QuickJS Mutex ensures only one thread
    /// enters the runtime at a time.
    fn setup_fetch(ctx: rquickjs::Ctx, has_network: bool) -> Result<(), rquickjs::Error> {
        if !has_network {
            ctx.eval::<(), _>(
                r#"globalThis.fetch = function() {
    return Promise.reject(new Error("PermissionError: 'network' permission required. Add \"network\" to your extension.json permissions list."));
};"#,
            )?;
            return Ok(());
        }
        // Native helper: _fetchRaw(url, method, body, headersJson)
        // Returns {status: i32, ok: bool, body: string} on success/HTTP-error.
        // Returns {status: 0, ok: false, body: "", error: string} on network failure.
        // Returns a JSON string to avoid rquickjs Object lifetime issues with 'static closures.
        // The JS side parses it back with JSON.parse() in the fetch() wrapper below.
        let fetch_raw_fn = rquickjs::Function::new(
            ctx.clone(),
            |_ctx: rquickjs::Ctx, args: Rest<rquickjs::Value>| {
                let url = args
                    .first()
                    .and_then(|v| v.clone().into_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default();

                let method = args
                    .get(1)
                    .filter(|v| !v.is_null() && !v.is_undefined())
                    .and_then(|v| v.clone().into_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| "GET".to_string());

                let body_str = args
                    .get(2)
                    .filter(|v| !v.is_null() && !v.is_undefined())
                    .and_then(|v| v.clone().into_string())
                    .and_then(|s| s.to_string().ok());

                let headers_json = args
                    .get(3)
                    .filter(|v| !v.is_null() && !v.is_undefined())
                    .and_then(|v| v.clone().into_string())
                    .and_then(|s| s.to_string().ok());

                // Build the ureq request.
                let mut request = ureq::request(&method, &url);
                if let Some(hj) = headers_json
                    && let Ok(serde_json::Value::Object(map)) =
                        serde_json::from_str::<serde_json::Value>(&hj)
                {
                    for (key, val) in map {
                        if let serde_json::Value::String(v) = val {
                            request = request.set(&key, &v);
                        }
                    }
                }

                // Execute the request (blocking).
                let (status, body, err_msg) = match body_str {
                    Some(b) => match request.send_string(&b) {
                        Ok(resp) => (resp.status(), resp.into_string().unwrap_or_default(), None),
                        Err(ureq::Error::Status(code, resp)) => {
                            (code, resp.into_string().unwrap_or_default(), None)
                        }
                        Err(e) => (0u16, String::new(), Some(e.to_string())),
                    },
                    None => match request.call() {
                        Ok(resp) => (resp.status(), resp.into_string().unwrap_or_default(), None),
                        Err(ureq::Error::Status(code, resp)) => {
                            (code, resp.into_string().unwrap_or_default(), None)
                        }
                        Err(e) => (0u16, String::new(), Some(e.to_string())),
                    },
                };

                // Serialize as JSON string; the JS wrapper parses it back.
                let result = serde_json::json!({
                    "status": status as i32,
                    "ok": (200u16..300u16).contains(&status),
                    "body": body,
                    "error": err_msg,
                });
                Ok::<String, rquickjs::Error>(result.to_string())
            },
        )?;

        ctx.globals().set("_fetchRaw", fetch_raw_fn)?;

        // Wrap _fetchRaw in a proper Promise-based fetch() API.
        ctx.eval::<(), _>(
            r#"
globalThis.fetch = function(url, opts) {
  return new Promise(function(resolve, reject) {
    try {
      var method = (opts && opts.method) ? String(opts.method).toUpperCase() : 'GET';
      var body = (opts && opts.body != null) ? String(opts.body) : null;
      var headersJson = (opts && opts.headers) ? JSON.stringify(opts.headers) : null;
      var raw = JSON.parse(globalThis._fetchRaw(url, method, body, headersJson));
      if (raw.error) { reject(new Error(raw.error)); return; }
      resolve({
        ok: raw.ok,
        status: raw.status,
        text: function() { return Promise.resolve(raw.body); },
        json: function() {
          try { return Promise.resolve(JSON.parse(raw.body)); }
          catch(e) { return Promise.reject(e); }
        }
      });
    } catch(e) {
      reject(new Error(String(e)));
    }
  });
};
        "#,
        )?;

        Ok(())
    }

    /// Drain the QuickJS microtask / Promise queue until it is empty.
    ///
    /// Must be called *outside* a `context.with()` closure so the runtime lock
    /// is available. Safe to call while the `Mutex<RuntimeCtx>` guard is held
    /// because that is our own lock, not the QuickJS runtime lock.
    fn pump_promise_queue(ctx_guard: &std::sync::MutexGuard<RuntimeCtx>) {
        loop {
            match ctx_guard.runtime.execute_pending_job() {
                Ok(true) => {}      // a microtask ran; loop in case it scheduled more
                Ok(false) => break, // queue empty
                Err(e) => {
                    eprintln!("Promise pump error: {e}");
                    break;
                }
            }
        }
    }

    /// Call `onSearch(query)` inside the persistent context and return captured results.
    async fn call_on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        // Clear previous results before the call.
        if let Ok(mut guard) = self.results.lock() {
            guard.clear();
        }

        let runtime_ctx = Arc::clone(&self.runtime_ctx);
        let results = Arc::clone(&self.results);
        let query = query.to_string();

        let task =
            tokio::task::spawn_blocking(move || -> Result<Vec<ExtensionItem>, ExtensionError> {
                use rquickjs::CatchResultExt;

                let ctx_guard = runtime_ctx
                    .lock()
                    .map_err(|e| ExtensionError::RuntimeError(format!("Mutex poisoned: {e}")))?;

                ctx_guard
                    .context
                    .with(|ctx| -> Result<(), ExtensionError> {
                        let global = ctx.globals();
                        let on_search: Result<rquickjs::Function, _> = global.get("onSearch");
                        if let Ok(func) = on_search {
                            let js_query = rquickjs::String::from_str(ctx.clone(), &query)
                                .map_err(|e| {
                                    ExtensionError::RuntimeError(format!("String error: {e}"))
                                })?;
                            func.call::<_, ()>((js_query,)).catch(&ctx).map_err(|e| {
                                ExtensionError::ExecutionError(format!("onSearch error: {e}"))
                            })?;
                        }
                        Ok(())
                    })?;

                // Pump the Promise queue so async extensions can resolve before we read results.
                Self::pump_promise_queue(&ctx_guard);

                Ok(results
                    .lock()
                    .map_err(|e| ExtensionError::RuntimeError(format!("Mutex poisoned: {e}")))?
                    .clone())
            });

        tokio::time::timeout(JS_CALL_TIMEOUT, task)
            .await
            .map_err(|_| ExtensionError::ExecutionError("on_search timed out (5 s)".to_string()))?
            .map_err(|e| ExtensionError::RuntimeError(format!("spawn_blocking join error: {e}")))?
    }

    /// Call `onAction(action, itemId)` inside the persistent context.
    async fn call_on_action(
        &self,
        action: &str,
        item_id: Option<&str>,
    ) -> Result<(), ExtensionError> {
        let runtime_ctx = Arc::clone(&self.runtime_ctx);
        let action = action.to_string();
        let item_id = item_id.map(str::to_string);

        let task = tokio::task::spawn_blocking(move || -> Result<(), ExtensionError> {
            use rquickjs::CatchResultExt;

            let ctx_guard = runtime_ctx
                .lock()
                .map_err(|e| ExtensionError::RuntimeError(format!("Mutex poisoned: {e}")))?;

            ctx_guard
                .context
                .with(|ctx| -> Result<(), ExtensionError> {
                    let global = ctx.globals();
                    let on_action: Result<rquickjs::Function, _> = global.get("onAction");
                    if let Ok(func) = on_action {
                        let js_action =
                            rquickjs::String::from_str(ctx.clone(), &action).map_err(|e| {
                                ExtensionError::RuntimeError(format!("String error: {e}"))
                            })?;

                        if let Some(ref id) = item_id {
                            let js_id =
                                rquickjs::String::from_str(ctx.clone(), id).map_err(|e| {
                                    ExtensionError::RuntimeError(format!("String error: {e}"))
                                })?;
                            func.call::<_, ()>((js_action, js_id))
                                .catch(&ctx)
                                .map_err(|e| {
                                    ExtensionError::ExecutionError(format!("onAction error: {e}"))
                                })?;
                        } else {
                            func.call::<_, ()>((js_action,)).catch(&ctx).map_err(|e| {
                                ExtensionError::ExecutionError(format!("onAction error: {e}"))
                            })?;
                        }
                    }
                    Ok(())
                })?;

            // Pump the Promise queue so async actions can complete.
            Self::pump_promise_queue(&ctx_guard);

            Ok(())
        });

        tokio::time::timeout(JS_CALL_TIMEOUT, task)
            .await
            .map_err(|_| ExtensionError::ExecutionError("on_action timed out (5 s)".to_string()))?
            .map_err(|e| ExtensionError::RuntimeError(format!("spawn_blocking join error: {e}")))?
    }
}

#[async_trait]
impl Extension for JsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        // Runtime and context are already initialised in `new()`.
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        self.call_on_search(query).await
    }

    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError> {
        self.call_on_action(action, item_id).await
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        // Runtime and context are cleaned up when JsExtension is dropped.
        Ok(())
    }

    /// Non-auto-load JS extensions expose a single launcher item so the user can
    /// discover and enter their dedicated mode from the global search list.
    fn launcher_item(&self) -> Option<crate::extension_trait::ExtensionItem> {
        if self.metadata.auto_load {
            return None;
        }
        let display_title = self
            .metadata
            .title
            .as_deref()
            .unwrap_or(&self.metadata.name);
        Some(crate::extension_trait::ExtensionItem {
            title: display_title.to_string(),
            subtitle: self.metadata.description.clone(),
            icon: Some("📋".to_string()),
            action: format!("enter-mode:{}", self.metadata.name),
            id: Some(format!("launcher:{}", self.metadata.name)),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
    }
}

/// Compute a hex-encoded hash of `data` using the named `algorithm`.
///
/// Supported algorithms: "md5", "sha1", "sha256", "sha512".
/// Unknown algorithms return an empty string.
pub(crate) fn crypto_hash(algorithm: &str, data: &str) -> String {
    use sha1::Digest as _;

    match algorithm.to_lowercase().as_str() {
        "md5" => {
            // md5 0.7 uses md5::compute() → Digest which formats as lowercase hex
            format!("{:x}", md5::compute(data.as_bytes()))
        }
        "sha1" => {
            let result = sha1::Sha1::digest(data.as_bytes());
            result.iter().map(|b| format!("{b:02x}")).collect()
        }
        "sha256" => {
            use sha2::Digest as _;
            let result = sha2::Sha256::digest(data.as_bytes());
            result.iter().map(|b| format!("{b:02x}")).collect()
        }
        "sha512" => {
            use sha2::Digest as _;
            let result = sha2::Sha512::digest(data.as_bytes());
            result.iter().map(|b| format!("{b:02x}")).collect()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_trait::{ExtensionLanguage, ExtensionMetadata};

    fn ext_js_metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            name: "ext_js".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Web search extension".to_string()),
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "ext_js.js".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        }
    }

    fn load_ext_js_code() -> &'static str {
        r#"
function onSearch(query) {
    if (!query || query === "") return;
    var results = [
        {
            title: "Search \"" + query + "\" on Google",
            subtitle: "Open Google search",
            action: "open-url:https://google.com/search?q=" + encodeURIComponent(query)
        },
        {
            title: "Search \"" + query + "\" on DuckDuckGo",
            subtitle: "Private search",
            action: "open-url:https://duckduckgo.com/?q=" + encodeURIComponent(query)
        }
    ];
    if (typeof raycast !== 'undefined') {
        raycast.updateList(results);
    }
}
"#
    }

    #[tokio::test]
    async fn empty_query_returns_no_results() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        let results = ext.on_search("").await.expect("on_search should not error");
        assert!(results.is_empty(), "empty query should produce no results");
    }

    #[tokio::test]
    async fn search_query_returns_two_results() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("rust")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 2, "should return exactly two search results");
    }

    #[tokio::test]
    async fn search_results_have_correct_titles() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("rust")
            .await
            .expect("on_search should not error");

        assert_eq!(results[0].title, r#"Search "rust" on Google"#);
        assert_eq!(results[1].title, r#"Search "rust" on DuckDuckGo"#);
    }

    #[tokio::test]
    async fn search_results_have_correct_actions() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("rust")
            .await
            .expect("on_search should not error");

        assert_eq!(
            results[0].action,
            "open-url:https://google.com/search?q=rust"
        );
        assert_eq!(results[1].action, "open-url:https://duckduckgo.com/?q=rust");
    }

    #[tokio::test]
    async fn query_is_url_encoded_in_actions() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("hello world")
            .await
            .expect("on_search should not error");

        assert_eq!(
            results[0].action,
            "open-url:https://google.com/search?q=hello%20world"
        );
        assert_eq!(
            results[1].action,
            "open-url:https://duckduckgo.com/?q=hello%20world"
        );
    }

    // ── console method tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn console_error_warn_debug_info_do_not_crash() {
        let code = r#"
function onSearch(query) {
    console.error("error msg");
    console.warn("warn msg");
    console.debug("debug msg");
    console.info("info msg");
    if (typeof raycast !== 'undefined') {
        raycast.updateList([]);
    }
}
"#;
        let ext = JsExtension::new(ext_js_metadata(), code, false)
            .await
            .expect("JsExtension should load");
        ext.on_search("test")
            .await
            .expect("on_search must not error when console.error/warn/debug/info are called");
    }

    // ── String.prototype.trim() regression ───────────────────────────────────

    /// QuickJS must support `String.prototype.trim()`.
    ///
    /// Extensions commonly branch on `query.trim() !== ""` to distinguish a
    /// blank search from a real one.  At one point the QuickJS binding silently
    /// lacked `trim()`, causing those branches to always fall through — a
    /// regression that showed up as two near-identical test extensions
    /// (`test_condition.js` using `.trim()` and `test_no_trim.js` without it).
    #[tokio::test]
    async fn string_trim_is_supported_in_quickjs() {
        let code = r#"
function onSearch(query) {
    var results;
    if (query && query.trim() !== "") {
        results = [{ title: "non-empty", action: "ok" }];
    } else {
        results = [{ title: "empty", action: "ok" }];
    }
    if (typeof raycast !== 'undefined') { raycast.updateList(results); }
}
globalThis.onSearch = onSearch;
"#;
        let ext = JsExtension::new(make_metadata("trim-test"), code, false)
            .await
            .expect("JsExtension should load");

        let r = ext.on_search("hello").await.expect("non-empty query");
        assert_eq!(r[0].title, "non-empty");

        let r = ext.on_search("").await.expect("empty query");
        assert_eq!(r[0].title, "empty");

        // Whitespace-only should also be treated as empty after trim().
        let r = ext.on_search("   ").await.expect("whitespace-only query");
        assert_eq!(r[0].title, "empty");
    }

    // ── fetch tests ──────────────────────────────────────────────────────────

    fn fetch_test_metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            name: "fetch-test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "fetch-test.js".to_string(),
            permissions: vec!["network".to_string()],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        }
    }

    /// Start a one-shot HTTP server on a random loopback port.
    ///
    /// The server accepts one connection, reads (and discards) the request,
    /// then sends `status` + `body` and closes.  Returns the bound port.
    fn local_http_server(status: u16, body: &'static str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let reason = if status == 200 { "OK" } else { "Error" };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 {status} {reason}\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {len}\r\n\
                     Connection: close\r\n\r\n{body}",
                    len = body.len(),
                )
                .as_bytes(),
            );
        });
        port
    }

    /// Happy path: `resp.json()` parses the response body and the result is
    /// surfaced through `raycast.updateList`.
    #[tokio::test]
    async fn fetch_json_resolves_from_local_server() {
        let port = local_http_server(200, r#"{"greeting":"pong"}"#);

        let js = format!(
            r#"
async function onSearch(query) {{
    const resp = await fetch('http://127.0.0.1:{port}');
    const data = await resp.json();
    raycast.updateList([{{ title: data.greeting, action: 'fetched' }}]);
}}
"#
        );
        let ext = JsExtension::new(fetch_test_metadata(), &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("ping")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "pong");
        assert_eq!(results[0].action, "fetched");
    }

    /// `resp.text()`, `resp.ok`, and `resp.status` are all accessible, and a
    /// non-200 status correctly sets `ok` to `false`.
    #[tokio::test]
    async fn fetch_text_ok_and_status_are_correct() {
        let port = local_http_server(404, r#"not found"#);

        let js = format!(
            r#"
async function onSearch(query) {{
    const resp = await fetch('http://127.0.0.1:{port}');
    const text = await resp.text();
    raycast.updateList([{{
        title: 'ok:' + resp.ok + ' status:' + resp.status + ' body:' + text,
        action: 'result'
    }}]);
}}
"#
        );
        let ext = JsExtension::new(fetch_test_metadata(), &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("check")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok:false status:404 body:not found");
    }

    /// A network-level error (connection immediately closed by peer) is
    /// surfaced as a rejected Promise and can be caught with try/catch.
    #[tokio::test]
    async fn fetch_network_error_is_caught_by_extension() {
        use std::net::TcpListener;

        // Accept the connection then drop the stream immediately — ureq will
        // see a connection-reset error, which fetch() turns into a rejection.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(stream); // close without sending anything
        });

        let js = format!(
            r#"
async function onSearch(query) {{
    try {{
        await fetch('http://127.0.0.1:{port}');
        raycast.updateList([{{ title: 'no-error', action: 'unexpected' }}]);
    }} catch (e) {{
        raycast.updateList([{{ title: 'caught-error', action: 'error' }}]);
    }}
}}
"#
        );
        let ext = JsExtension::new(fetch_test_metadata(), &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("test")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "caught-error");
    }

    // ── detail field tests ────────────────────────────────────────────────────

    fn detail_test_metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            name: "detail-test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "detail-test.js".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        }
    }

    #[tokio::test]
    async fn js_extension_can_return_detail_field() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{
        title: 'item with detail',
        action: 'noop',
        detail: 'This is the full detail content.'
    }]);
}
"#;
        let ext = JsExtension::new(detail_test_metadata(), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].detail.as_deref(),
            Some("This is the full detail content.")
        );
    }

    #[tokio::test]
    async fn js_extension_detail_is_none_when_not_returned() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{ title: 'no detail', action: 'noop' }]);
}
"#;
        let ext = JsExtension::new(detail_test_metadata(), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert!(results[0].detail.is_none());
    }

    #[tokio::test]
    async fn persistent_context_reused_across_multiple_searches() {
        let code = load_ext_js_code();
        let ext = JsExtension::new(ext_js_metadata(), &code, false)
            .await
            .expect("JsExtension should load");

        // Call on_search multiple times — results must be independent.
        let r1 = ext.on_search("rust").await.expect("first search");
        let r2 = ext.on_search("hello world").await.expect("second search");
        let r3 = ext.on_search("").await.expect("empty search");

        assert_eq!(r1.len(), 2);
        assert_eq!(r2.len(), 2);
        assert!(r3.is_empty());

        // Make sure the first results weren't polluted by the second call.
        assert!(r1[0].action.contains("rust"));
        assert!(r2[0].action.contains("hello%20world"));
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Generic metadata builder — avoids repeating boilerplate in every test.
    fn make_metadata(name: &str) -> ExtensionMetadata {
        ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: format!("{name}.js"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        }
    }

    // ── onAction invocation ───────────────────────────────────────────────────

    /// JS `onAction` receives the correct action string.
    /// The extension stores it in a module variable that a follow-up
    /// `onSearch("__check__")` returns as a result title.
    #[tokio::test]
    async fn on_action_receives_correct_action_string() {
        let js = r#"
var lastAction = null;
function onSearch(query) {
    if (query === '__check__') {
        raycast.updateList([{ title: lastAction || 'none', action: 'ok' }]);
    }
}
function onAction(action, itemId) {
    lastAction = action;
}
"#;
        let ext = JsExtension::new(make_metadata("action-str"), js, false)
            .await
            .expect("JsExtension should load");

        ext.on_action("my-custom-action", None)
            .await
            .expect("on_action should not error");

        let results = ext
            .on_search("__check__")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "my-custom-action");
    }

    /// JS `onAction` receives the item_id as its second argument.
    #[tokio::test]
    async fn on_action_passes_item_id_to_js() {
        let js = r#"
var lastItemId = null;
function onSearch(query) {
    if (query === '__check__') {
        raycast.updateList([{ title: lastItemId || 'none', action: 'ok' }]);
    }
}
function onAction(action, itemId) {
    lastItemId = itemId;
}
"#;
        let ext = JsExtension::new(make_metadata("action-id"), js, false)
            .await
            .expect("JsExtension should load");

        ext.on_action("some-action", Some("item-42"))
            .await
            .expect("on_action should not error");

        let results = ext
            .on_search("__check__")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "item-42");
    }

    /// Calling on_action without an item_id (None) does not error.
    #[tokio::test]
    async fn on_action_without_item_id_does_not_error() {
        let js = r#"
function onSearch(query) {}
function onAction(action, itemId) {}
"#;
        let ext = JsExtension::new(make_metadata("action-noid"), js, false)
            .await
            .expect("JsExtension should load");

        let result = ext.on_action("test-action", None).await;
        assert!(result.is_ok(), "on_action with no item_id should succeed");
    }

    /// If the extension doesn't define `onAction`, calling it succeeds silently.
    #[tokio::test]
    async fn on_action_silently_skipped_when_not_defined() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{ title: 'hello', action: 'ok' }]);
}
// No onAction defined
"#;
        let ext = JsExtension::new(make_metadata("no-action"), js, false)
            .await
            .expect("JsExtension should load");

        let result = ext.on_action("test-action", Some("item-1")).await;
        assert!(result.is_ok(), "missing onAction should not cause an error");
    }

    /// Module-level state persists across on_search → on_action → on_search.
    /// This is the core pattern real Raycast extensions use: store item data
    /// during search, then read it back in the action handler.
    #[tokio::test]
    async fn module_state_persists_between_on_search_and_on_action() {
        let js = r#"
var cachedData = null;
var actionSaw = null;
globalThis.onSearch = function(query) {
    if (query === '__verify__') {
        raycast.updateList([{ title: actionSaw || 'none', action: 'ok' }]);
        return;
    }
    if (!query) return;
    cachedData = 'data-for-' + query;
    raycast.updateList([{ title: cachedData, action: 'use-cache', id: 'item1' }]);
};
globalThis.onAction = function(action, itemId) {
    actionSaw = cachedData;
};
"#;
        let ext = JsExtension::new(make_metadata("state-test"), js, false)
            .await
            .expect("JsExtension should load");

        // Search populates cachedData.
        let r1 = ext.on_search("hello").await.expect("first search");
        assert_eq!(r1[0].title, "data-for-hello");

        // Action reads cachedData into actionSaw.
        ext.on_action("use-cache", Some("item1"))
            .await
            .expect("action");

        // A follow-up search confirms actionSaw was set correctly by onAction.
        let r2 = ext.on_search("__verify__").await.expect("verify search");
        assert_eq!(r2[0].title, "data-for-hello");
    }

    // ── optional item fields ──────────────────────────────────────────────────

    /// All optional ExtensionItem fields (subtitle, icon, id) are forwarded
    /// from the JS object to the Rust struct by raycast.updateList().
    #[tokio::test]
    async fn all_optional_item_fields_flow_from_js() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{
        title: 'My Item',
        subtitle: 'A subtitle',
        icon: '🚀',
        action: 'test-action',
        id: 'item-001',
    }]);
}
"#;
        let ext = JsExtension::new(make_metadata("fields-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("q")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subtitle.as_deref(), Some("A subtitle"));
        assert_eq!(results[0].icon.as_deref(), Some("🚀"));
        assert_eq!(results[0].id.as_deref(), Some("item-001"));
    }

    /// `extraActions` array in an item JS object flows through to `extra_actions` on the Rust struct.
    #[tokio::test]
    async fn extra_actions_flow_from_js_to_rust() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{
        title: 'My Item',
        action: 'primary-action',
        extraActions: [
            { title: 'Copy', action: 'clipboard-copy:hello', icon: '📋', shortcut: '⌘C' },
            { title: 'Open',  action: 'open-url:https://example.com', shortcut: null },
        ],
    }]);
}
"#;
        let ext = JsExtension::new(make_metadata("extra-actions-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("q")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        let extra = &results[0].extra_actions;
        assert_eq!(extra.len(), 2, "expected 2 extra actions; got {extra:?}");
        assert_eq!(extra[0].title, "Copy");
        assert_eq!(extra[0].action, "clipboard-copy:hello");
        assert_eq!(extra[0].icon.as_deref(), Some("📋"));
        assert_eq!(extra[0].shortcut.as_deref(), Some("⌘C"));
        assert_eq!(extra[1].title, "Open");
        assert_eq!(extra[1].action, "open-url:https://example.com");
        assert!(extra[1].shortcut.is_none());
    }

    /// Items without `extraActions` have an empty `extra_actions` vec.
    #[tokio::test]
    async fn items_without_extra_actions_have_empty_vec() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{ title: 'Simple', action: 'noop' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("no-extra-actions"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("q")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert!(results[0].extra_actions.is_empty());
    }

    /// When optional fields are absent the Rust struct has None for each.
    #[tokio::test]
    async fn items_with_only_required_fields_have_none_for_optionals() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{ title: 'Minimal', action: 'ok' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("minimal-item"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("q")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert!(results[0].subtitle.is_none());
        assert!(results[0].icon.is_none());
        assert!(results[0].id.is_none());
    }

    // ── error handling ────────────────────────────────────────────────────────

    /// A JavaScript syntax error at load time causes JsExtension::new() to fail.
    #[tokio::test]
    async fn syntax_error_in_js_fails_to_load() {
        let js = "function onSearch( { this is not valid javascript {{{{";
        let result = JsExtension::new(make_metadata("bad-js"), js, false).await;
        assert!(result.is_err(), "invalid JS should fail to load");
    }

    /// When onSearch throws a JS exception, on_search() returns Err.
    #[tokio::test]
    async fn runtime_throw_in_on_search_returns_error() {
        let js = r#"
function onSearch(query) {
    throw new Error("intentional failure");
}
"#;
        let ext = JsExtension::new(make_metadata("throws"), js, false)
            .await
            .expect("JsExtension should load");

        let result = ext.on_search("q").await;
        assert!(
            result.is_err(),
            "on_search should propagate JS exceptions as Err"
        );
    }

    /// If the extension doesn't define onSearch, on_search() returns Ok([]).
    #[tokio::test]
    async fn extension_without_on_search_returns_empty() {
        let js = r#"
// Extension with no onSearch defined
var version = "1.0";
"#;
        let ext = JsExtension::new(make_metadata("no-search"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext.on_search("anything").await.expect("should not error");
        assert!(
            results.is_empty(),
            "missing onSearch should return empty results"
        );
    }

    // ── JS standard library ───────────────────────────────────────────────────

    /// console.log() with mixed argument types runs without panicking.
    #[tokio::test]
    async fn console_log_does_not_panic() {
        let js = r#"
function onSearch(query) {
    console.log('called with:', query, 42, true, null);
    raycast.updateList([{ title: 'ok', action: 'ok' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("console-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("test")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
    }

    /// JSON.stringify and JSON.parse are available — critical for API call patterns.
    #[tokio::test]
    async fn json_stringify_and_parse_are_available() {
        let js = r#"
function onSearch(query) {
    var obj = { name: query, count: 3 };
    var json = JSON.stringify(obj);
    var parsed = JSON.parse(json);
    raycast.updateList([{ title: parsed.name + ':' + parsed.count, action: 'ok' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("json-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("hello")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "hello:3");
    }

    /// Array.prototype.map and filter are available — used in almost every extension.
    #[tokio::test]
    async fn array_map_and_filter_work() {
        let js = r#"
function onSearch(query) {
    var items = ['alpha', 'beta', 'gamma']
        .filter(function(s) { return s.startsWith(query); })
        .map(function(s) { return { title: s, action: 'pick:' + s }; });
    raycast.updateList(items);
}
"#;
        let ext = JsExtension::new(make_metadata("array-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("a")
            .await
            .expect("on_search should not error");
        // Only 'alpha' starts with 'a'.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "alpha");
    }

    // ── updateList behaviour ──────────────────────────────────────────────────

    /// If `raycast.updateList` is called twice in one `onSearch`, the second
    /// call overwrites the first — results are not accumulated.
    #[tokio::test]
    async fn update_list_called_twice_last_call_wins() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([{ title: 'first', action: 'a' }]);
    raycast.updateList([{ title: 'second', action: 'b' }, { title: 'third', action: 'c' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("double-update"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");
        assert_eq!(
            results.len(),
            2,
            "second updateList call should overwrite the first"
        );
        assert_eq!(results[0].title, "second");
        assert_eq!(results[1].title, "third");
    }

    /// Items passed to `raycast.updateList` that are missing the required
    /// `title` or `action` fields are silently skipped; valid items still appear.
    #[tokio::test]
    async fn item_missing_required_field_is_silently_dropped() {
        let js = r#"
function onSearch(query) {
    raycast.updateList([
        { title: 'valid', action: 'ok' },
        { title: 'no-action' },
        { action: 'no-title' },
        {},
        { title: 'also-valid', action: 'ok2' },
    ]);
}
"#;
        let ext = JsExtension::new(make_metadata("drop-invalid"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");
        assert_eq!(
            results.len(),
            2,
            "only items with both title and action should be kept"
        );
        assert_eq!(results[0].title, "valid");
        assert_eq!(results[1].title, "also-valid");
    }

    /// When `onAction` throws a JS exception, `on_action()` returns `Err`.
    #[tokio::test]
    async fn on_action_that_throws_returns_err() {
        let js = r#"
function onSearch(query) {}
function onAction(action, itemId) {
    throw new Error("action failed");
}
"#;
        let ext = JsExtension::new(make_metadata("action-throws"), js, false)
            .await
            .expect("JsExtension should load");

        let result = ext.on_action("test", None).await;
        assert!(
            result.is_err(),
            "on_action should propagate JS exceptions as Err"
        );
    }

    /// Unicode characters and emoji in the query are passed through intact.
    #[tokio::test]
    async fn unicode_query_is_passed_through_intact() {
        let js = r#"
function onSearch(query) {
    if (!query) return;
    raycast.updateList([{ title: query, action: 'ok' }]);
}
"#;
        let ext = JsExtension::new(make_metadata("unicode-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("日本語テスト 🦀")
            .await
            .expect("unicode query should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "日本語テスト 🦀");
    }

    /// Template literals (backtick strings with interpolation) work in extensions.
    #[tokio::test]
    async fn template_literals_work() {
        let js = r#"
function onSearch(query) {
    if (!query) return;
    raycast.updateList([{
        title: `Result for "${query}"`,
        subtitle: `${query.length} chars`,
        action: `open-url:https://example.com?q=${encodeURIComponent(query)}`
    }]);
}
"#;
        let ext = JsExtension::new(make_metadata("template-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("hi")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, r#"Result for "hi""#);
        assert_eq!(results[0].subtitle.as_deref(), Some("2 chars"));
    }

    /// Arrow functions assigned to globalThis work as extension entry points.
    #[tokio::test]
    async fn arrow_functions_work() {
        let js = r#"
globalThis.onSearch = (query) => {
    if (!query) return;
    var items = ['one', 'two', 'three']
        .filter(s => s.includes(query))
        .map(s => ({ title: s, action: `pick:${s}` }));
    raycast.updateList(items);
};
"#;
        let ext = JsExtension::new(make_metadata("arrow-test"), js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("o")
            .await
            .expect("on_search should not error");
        // 'one' and 'two' contain 'o'; 'three' does not.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "one");
        assert_eq!(results[1].title, "two");
    }

    // ── fetch: POST and headers ───────────────────────────────────────────────

    /// fetch() with method: 'POST', body, and Content-Type header succeeds.
    #[tokio::test]
    async fn fetch_post_with_body_succeeds() {
        let port = local_http_server(200, r#"{"received":true}"#);

        let js = format!(
            r#"
async function onSearch(query) {{
    const resp = await fetch('http://127.0.0.1:{port}', {{
        method: 'POST',
        body: JSON.stringify({{ query: query }}),
        headers: {{ 'Content-Type': 'application/json' }}
    }});
    const data = await resp.json();
    raycast.updateList([{{
        title: 'ok:' + resp.ok + ' received:' + data.received,
        action: 'result'
    }}]);
}}
"#
        );
        let ext = JsExtension::new(fetch_test_metadata(), &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("test")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok:true received:true");
    }

    // ── permission enforcement ────────────────────────────────────────────────

    /// Without the "network" permission, calling fetch() rejects with PermissionError.
    #[tokio::test]
    async fn fetch_without_network_permission_rejects_with_permission_error() {
        let meta = ExtensionMetadata {
            name: "no-network".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "no-network.js".to_string(),
            permissions: vec![], // intentionally empty
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        let js = r#"
async function onSearch(query) {
    try {
        await fetch('http://127.0.0.1:1');
        raycast.updateList([{ title: 'no-error', action: 'unexpected' }]);
    } catch (e) {
        raycast.updateList([{ title: String(e), action: 'caught' }]);
    }
}
"#;
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");
        assert_eq!(
            results.len(),
            1,
            "permission error should be caught by extension"
        );
        assert!(
            results[0].title.contains("PermissionError"),
            "error message should mention PermissionError, got: {}",
            results[0].title
        );
        assert!(
            results[0].title.contains("network"),
            "error message should mention 'network', got: {}",
            results[0].title
        );
    }

    /// With the "network" permission declared, fetch() resolves normally.
    #[tokio::test]
    async fn fetch_with_network_permission_resolves_normally() {
        let port = local_http_server(200, r#"{"ok":true}"#);

        let meta = ExtensionMetadata {
            name: "with-network".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "with-network.js".to_string(),
            permissions: vec!["network".to_string()],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        let js = format!(
            r#"
async function onSearch(query) {{
    const resp = await fetch('http://127.0.0.1:{port}');
    raycast.updateList([{{ title: 'status:' + resp.status, action: 'ok' }}]);
}}
"#
        );
        let ext = JsExtension::new(meta, &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("x")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "status:200");
    }

    /// fetch() with custom headers (Authorization, X-Custom-Header) succeeds.
    #[tokio::test]
    async fn fetch_with_custom_headers_succeeds() {
        let port = local_http_server(200, r#"{"ok":true}"#);

        let js = format!(
            r#"
async function onSearch(query) {{
    const resp = await fetch('http://127.0.0.1:{port}', {{
        headers: {{
            'Authorization': 'Bearer test-token',
            'X-Custom-Header': 'value'
        }}
    }});
    const data = await resp.json();
    raycast.updateList([{{ title: 'status:' + resp.status, action: 'ok' }}]);
}}
"#
        );
        let ext = JsExtension::new(fetch_test_metadata(), &js, false)
            .await
            .expect("JsExtension should load");

        let results = ext
            .on_search("test")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "status:200");
    }

    // ── showToast bridge ──────────────────────────────────────────────────────

    /// `raycast.showToast` called from JS sends `ExtensionMessage::ShowToast`
    /// through the channel when the extension is created via `new_with_sender`.
    #[tokio::test]
    async fn show_toast_sends_message_via_sender() {
        let (sender, receiver) =
            crossbeam_channel::unbounded::<crate::extension_manager::ExtensionMessage>();

        let js = r#"
function onSearch(query) {
    raycast.showToast("success", "Done", "saved");
}
"#;
        let ext = JsExtension::new_with_sender(make_metadata("toast-test"), js, false, sender)
            .await
            .expect("JsExtension should load");

        ext.on_search("").await.expect("on_search should not error");

        let msg = receiver
            .try_recv()
            .expect("ShowToast message should have been sent");
        match msg {
            crate::extension_manager::ExtensionMessage::ShowToast(style, title, message) => {
                assert_eq!(style, "success");
                assert_eq!(title, "Done");
                assert_eq!(message, "saved");
            }
            other => panic!("Expected ShowToast, got: {other:?}"),
        }
    }

    // ── crypto_hash tests ────────────────────────────────────────────────────
    #[test]
    fn crypto_hash_md5_empty() {
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(
            super::crypto_hash("md5", ""),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }

    #[test]
    fn crypto_hash_md5_hello() {
        // md5("hello") = 5d41402abc4b2a76b9719d911017c592
        assert_eq!(
            super::crypto_hash("md5", "hello"),
            "5d41402abc4b2a76b9719d911017c592"
        );
    }

    #[test]
    fn crypto_hash_sha1() {
        // sha1("") = da39a3ee5e6b4b0d3255bfef95601890afd80709
        assert_eq!(
            super::crypto_hash("sha1", ""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn crypto_hash_sha256() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            super::crypto_hash("sha256", ""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn crypto_hash_sha256_hello() {
        assert_eq!(
            super::crypto_hash("sha256", "hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn crypto_hash_sha512() {
        // sha512("") known value
        assert_eq!(
            super::crypto_hash("sha512", ""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn crypto_hash_unknown_returns_empty() {
        assert_eq!(super::crypto_hash("blake2", "hello"), "");
    }

    #[test]
    fn crypto_hash_case_insensitive() {
        assert_eq!(
            super::crypto_hash("MD5", "hello"),
            super::crypto_hash("md5", "hello")
        );
    }
}

// ── @raycast/api shim tests ───────────────────────────────────────────────────
//
// These tests verify the Phase-1 Raycast compatibility layer: CJS-transformed
// extensions that import from `@raycast/api` and export a React default component.
// The shim is injected, the default export is bootstrapped, and `onSearch`
// is wired to the List component's `onSearchTextChange`.

#[cfg(test)]
mod raycast_shim_tests {
    use crate::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
    use crate::js_extension::JsExtension;
    use crate::transpiler::transpile_for_raycast;

    fn raycast_meta(name: &str) -> ExtensionMetadata {
        ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::TypeScript,
            entry_point: format!("{name}.tsx"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        }
    }

    /// Helper: transpile TSX and load as a Raycast CJS extension.
    async fn load_raycast(name: &str, tsx: &str) -> JsExtension {
        let file = format!("{name}.tsx");
        let cjs =
            transpile_for_raycast(tsx, &file).expect("transpile_for_raycast should not error");
        JsExtension::new(raycast_meta(name), &cjs, true)
            .await
            .expect("JsExtension::new in Raycast mode should not error")
    }

    // ── Basic rendering ───────────────────────────────────────────────────────

    /// A minimal Raycast extension with one static List.Item renders correctly.
    #[tokio::test]
    async fn static_list_item_renders_as_extension_item() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item
        title="Hello Raycast"
        subtitle="A subtitle"
        actions={
          <ActionPanel>
            <Action.OpenInBrowser url="https://example.com" />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("static-item", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 1, "should render exactly one item");
        assert_eq!(results[0].title, "Hello Raycast");
        assert_eq!(results[0].subtitle.as_deref(), Some("A subtitle"));
        assert_eq!(results[0].action, "open-url:https://example.com");
    }

    /// Action.CopyToClipboard maps to `clipboard-copy:<content>`.
    #[tokio::test]
    async fn copy_to_clipboard_action_maps_to_action_string() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item
        title="Copy me"
        actions={
          <ActionPanel>
            <Action.CopyToClipboard content="clipboard text" />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("copy-action", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "clipboard-copy:clipboard text");
    }

    /// Multiple List.Items all appear in the result list.
    #[tokio::test]
    async fn multiple_list_items_all_rendered() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item title="First" actions={<ActionPanel><Action.OpenInBrowser url="https://one.com" /></ActionPanel>} />
      <List.Item title="Second" actions={<ActionPanel><Action.OpenInBrowser url="https://two.com" /></ActionPanel>} />
      <List.Item title="Third" actions={<ActionPanel><Action.OpenInBrowser url="https://three.com" /></ActionPanel>} />
    </List>
  );
}
"#;
        let ext = load_raycast("multi-item", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].title, "First");
        assert_eq!(results[1].title, "Second");
        assert_eq!(results[2].title, "Third");
    }

    // ── useState + onSearchTextChange ─────────────────────────────────────────

    /// List with onSearchTextChange: items update as the query changes.
    #[tokio::test]
    async fn search_text_change_triggers_re_render() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";
import { useState } from "@raycast/api";

export default function Command() {
  const [query, setQuery] = useState("");
  return (
    <List onSearchTextChange={setQuery}>
      {query ? (
        <List.Item
          title={"Result for: " + query}
          actions={<ActionPanel><Action.OpenInBrowser url={"https://example.com?q=" + query} /></ActionPanel>}
        />
      ) : null}
    </List>
  );
}
"#;
        let ext = load_raycast("search-text", tsx).await;

        // Empty search → no items
        let r0 = ext.on_search("").await.expect("empty search");
        assert_eq!(r0.len(), 0, "empty query should yield no items");

        // Non-empty search → one item with the query in its title
        let r1 = ext.on_search("hello").await.expect("search hello");
        assert_eq!(r1.len(), 1, "non-empty query should yield one item");
        assert_eq!(r1[0].title, "Result for: hello");
        assert_eq!(r1[0].action, "open-url:https://example.com?q=hello");

        // Second different query also updates
        let r2 = ext.on_search("world").await.expect("search world");
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].title, "Result for: world");
    }

    // ── transpile_for_raycast unit tests ──────────────────────────────────────

    /// TSX with JSX is correctly compiled to CJS with `require` calls.
    #[test]
    fn raycast_transpile_converts_jsx_to_cjs() {
        let tsx = r#"
import { List } from "@raycast/api";
export default function Cmd() {
  return <List />;
}
"#;
        let js = transpile_for_raycast(tsx, "cmd.tsx").unwrap();
        assert!(
            js.contains("require"),
            "CJS output should use require(): {js}"
        );
        assert!(
            js.contains("@raycast/api"),
            "output should reference @raycast/api: {js}"
        );
        assert!(
            js.contains("exports"),
            "CJS output should use exports: {js}"
        );
    }

    /// Plain JS extensions (no @raycast/api import) are NOT treated as Raycast mode.
    #[test]
    fn non_raycast_js_not_detected_as_raycast() {
        use crate::transpiler::is_raycast_api_extension;
        let js = r#"
function onSearch(query) {
  raycast.updateList([{ title: query, action: 'ok' }]);
}
"#;
        assert!(
            !is_raycast_api_extension(js),
            "legacy JS should not be detected as Raycast mode"
        );
    }

    /// Code with `@raycast/api` IS detected as a Raycast extension.
    #[test]
    fn raycast_api_import_is_detected() {
        use crate::transpiler::is_raycast_api_extension;
        let tsx = r#"import { List } from "@raycast/api";"#;
        assert!(
            is_raycast_api_extension(tsx),
            "code with @raycast/api import should be detected as Raycast mode"
        );
    }

    // ── @raycast/utils shim ───────────────────────────────────────────────────

    /// Importing from `@raycast/utils` must not throw.
    #[tokio::test]
    async fn raycast_utils_import_does_not_throw() {
        let tsx = r#"
import { List } from "@raycast/api";
import { getAvatarIcon } from "@raycast/utils";

export default function Command() {
  return (
    <List>
      <List.Item title={getAvatarIcon("Alice")} />
    </List>
  );
}
"#;
        let ext = load_raycast("utils-import", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        // getAvatarIcon("Alice") → "A"
        assert_eq!(results[0].title, "A");
    }

    /// `showFailureToast` must be callable without crashing.
    #[tokio::test]
    async fn raycast_utils_show_failure_toast_does_not_crash() {
        let tsx = r#"
import { List } from "@raycast/api";
import { showFailureToast } from "@raycast/utils";

export default function Command() {
  showFailureToast("Something went wrong");
  return (
    <List>
      <List.Item title="ok" />
    </List>
  );
}
"#;
        let ext = load_raycast("utils-toast", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok");
    }

    /// `useCachedPromise` must return the same shape as `usePromise`.
    #[tokio::test]
    async fn raycast_utils_use_cached_promise_returns_data() {
        let tsx = r#"
import { List } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";

async function fetchData() {
  return "cached-result";
}

export default function Command() {
  const { data, isLoading } = useCachedPromise(fetchData, []);
  return (
    <List>
      <List.Item title={isLoading ? "loading" : (data || "no-data")} />
    </List>
  );
}
"#;
        let ext = load_raycast("utils-cached-promise", tsx).await;
        // First render shows loading state (promise not yet resolved in sync render)
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        // Either "loading" or "cached-result" is acceptable depending on promise timing
        assert!(
            results[0].title == "loading" || results[0].title == "cached-result",
            "unexpected title: {}",
            results[0].title
        );
    }

    // ── Node.js built-in stubs ────────────────────────────────────────────────

    /// `require('path').join` must concatenate path segments.
    #[tokio::test]
    async fn node_path_join_works() {
        let tsx = r#"
import { List } from "@raycast/api";
const path = require("path");

export default function Command() {
  const joined = path.join("/home", "user", "docs");
  return (
    <List>
      <List.Item title={joined} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-path-join", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "/home/user/docs");
    }

    // ── accessories tests ─────────────────────────────────────────────────────

    /// List.Item accessories with `.text` are collected into the accessories vec.
    #[tokio::test]
    async fn list_item_text_accessories_collected() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item
        title="item"
        accessories={[{ text: "label1" }, { text: "label2" }]}
      />
    </List>
  );
}
"#;
        let ext = load_raycast("accessories-text", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].accessories, vec!["label1", "label2"]);
    }

    /// List.EmptyView emits a sentinel item with id "::empty::".
    #[tokio::test]
    async fn list_empty_view_emits_sentinel() {
        let tsx = r#"
import { List } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.EmptyView title="Nothing found" description="Try a different query" />
    </List>
  );
}
"#;
        let ext = load_raycast("list-emptyview", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1, "should emit exactly one sentinel item");
        assert_eq!(results[0].id.as_deref(), Some("::empty::"));
        assert_eq!(results[0].action, "::empty::");
        assert_eq!(results[0].title, "Nothing found");
        assert_eq!(
            results[0].subtitle.as_deref(),
            Some("Try a different query")
        );
    }

    /// `require('path').basename` must extract the file name.
    #[tokio::test]
    async fn node_path_basename_works() {
        let tsx = r#"
import { List } from "@raycast/api";
const path = require("path");

export default function Command() {
  const name = path.basename("/home/user/file.txt", ".txt");
  return (
    <List>
      <List.Item title={name} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-path-basename", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "file");
    }

    /// `require('os').platform` must return a platform string without throwing.
    #[tokio::test]
    async fn node_os_platform_does_not_throw() {
        let tsx = r#"
import { List } from "@raycast/api";
const os = require("os");

export default function Command() {
  const p = os.platform();
  return (
    <List>
      <List.Item title={p || "unknown"} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-os-platform", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        let title = &results[0].title;
        assert!(
            title == "linux" || title == "darwin" || title == "win32",
            "unexpected platform: {title}"
        );
    }

    /// `require('os').homedir` must return a non-empty string.
    #[tokio::test]
    async fn node_os_homedir_returns_string() {
        let tsx = r#"
import { List } from "@raycast/api";
const os = require("os");

export default function Command() {
  const h = os.homedir();
  return (
    <List>
      <List.Item title={h ? "ok" : "empty"} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-os-homedir", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok");
    }

    /// `require('querystring').stringify` must produce a query string.
    #[tokio::test]
    async fn node_querystring_stringify_works() {
        let tsx = r#"
import { List } from "@raycast/api";
const qs = require("querystring");

export default function Command() {
  const str = qs.stringify({ a: "1", b: "2" });
  return (
    <List>
      <List.Item title={str} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-querystring", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        let title = &results[0].title;
        assert!(
            title.contains("a=1") && title.contains("b=2"),
            "unexpected: {title}"
        );
    }

    // ── List.Section tests ────────────────────────────────────────────────────

    /// List.Section emits a section-header sentinel item followed by its children.
    #[tokio::test]
    async fn list_section_emits_header_and_children() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Section title="Group A">
        <List.Item title="item-a1" />
        <List.Item title="item-a2" />
      </List.Section>
      <List.Section title="Group B">
        <List.Item title="item-b1" />
      </List.Section>
    </List>
  );
}
"#;
        let ext = load_raycast("list-section", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        // Expected: header-A, item-a1, item-a2, header-B, item-b1
        assert_eq!(results.len(), 5, "should have 2 headers + 3 items");

        // Section headers have action "::section::"
        assert_eq!(
            results[0].action, "::section::",
            "first item should be a section header"
        );
        assert_eq!(results[0].title, "Group A");
        assert_eq!(results[1].title, "item-a1");
        assert_eq!(results[2].title, "item-a2");
        assert_eq!(
            results[3].action, "::section::",
            "fourth item should be second section header"
        );
        assert_eq!(results[3].title, "Group B");
        assert_eq!(results[4].title, "item-b1");
    }

    /// `require('url').URL` must parse a URL without throwing.
    #[tokio::test]
    async fn node_url_class_works() {
        let tsx = r#"
import { List } from "@raycast/api";
const { URL } = require("url");

export default function Command() {
  const u = new URL("https://example.com/path?q=1");
  return (
    <List>
      <List.Item title={u.hostname} />
    </List>
  );
}
"#;
        let ext = load_raycast("node-url", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "example.com");
    }

    // ── storage / LocalStorage / environment.supportPath tests ───────────────

    fn storage_test_dir(ext_name: &str) -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".pterry")
            .join("extension-data")
            .join(ext_name)
    }

    fn clean_storage(ext_name: &str) {
        let _ = std::fs::remove_dir_all(storage_test_dir(ext_name));
    }

    /// `raycast.storageGet` returns null for a key that has never been stored.
    #[tokio::test]
    async fn storage_get_returns_null_for_unknown_key() {
        let name = "storage-get-null";
        clean_storage(name);
        let js = r#"
function onSearch(query) {
    var val = raycast.storageGet("nonexistent");
    var label = (val === null || val === undefined) ? "null" : String(val);
    raycast.updateList([{ title: label, action: "ok" }]);
}
"#;
        let meta = ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: format!("{name}.js"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("should load");
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results[0].title, "null");
    }

    /// `raycast.storageSet` followed by `raycast.storageGet` returns the stored value.
    #[tokio::test]
    async fn storage_set_then_get_roundtrip() {
        let name = "storage-roundtrip";
        clean_storage(name);
        let js = r#"
function onSearch(query) {
    if (query === "set") {
        raycast.storageSet("testkey", "testvalue");
        raycast.updateList([{ title: "set-done", action: "ok" }]);
    } else {
        var val = raycast.storageGet("testkey");
        raycast.updateList([{ title: val || "null", action: "ok" }]);
    }
}
"#;
        let meta = ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: format!("{name}.js"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("should load");
        ext.on_search("set")
            .await
            .expect("set phase should not error");
        let results = ext
            .on_search("get")
            .await
            .expect("get phase should not error");
        assert_eq!(results[0].title, "testvalue");
    }

    /// `raycast.storageDel` removes a previously stored key.
    #[tokio::test]
    async fn storage_del_removes_key() {
        let name = "storage-del";
        clean_storage(name);
        let js = r#"
function onSearch(query) {
    if (query === "set") {
        raycast.storageSet("delkey", "exists");
        raycast.updateList([{ title: "set", action: "ok" }]);
    } else if (query === "del") {
        raycast.storageDel("delkey");
        raycast.updateList([{ title: "del", action: "ok" }]);
    } else {
        var val = raycast.storageGet("delkey");
        raycast.updateList([{ title: (val === null || val === undefined) ? "null" : val, action: "ok" }]);
    }
}
"#;
        let meta = ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: format!("{name}.js"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("should load");
        ext.on_search("set")
            .await
            .expect("set phase should not error");
        ext.on_search("del")
            .await
            .expect("del phase should not error");
        let results = ext
            .on_search("check")
            .await
            .expect("check phase should not error");
        assert_eq!(results[0].title, "null");
    }

    /// `raycast.storageClear` removes all stored keys.
    #[tokio::test]
    async fn storage_clear_removes_all_keys() {
        let name = "storage-clear";
        clean_storage(name);
        let js = r#"
function onSearch(query) {
    if (query === "set") {
        raycast.storageSet("k1", "v1");
        raycast.storageSet("k2", "v2");
        raycast.updateList([{ title: "set", action: "ok" }]);
    } else if (query === "clear") {
        raycast.storageClear();
        raycast.updateList([{ title: "clear", action: "ok" }]);
    } else {
        var v1 = raycast.storageGet("k1");
        raycast.updateList([{ title: (v1 === null || v1 === undefined) ? "null" : v1, action: "ok" }]);
    }
}
"#;
        let meta = ExtensionMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: format!("{name}.js"),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("should load");
        ext.on_search("set")
            .await
            .expect("set phase should not error");
        ext.on_search("clear")
            .await
            .expect("clear phase should not error");
        let results = ext
            .on_search("check")
            .await
            .expect("check phase should not error");
        assert_eq!(results[0].title, "null");
    }

    /// `environment.supportPath` is a non-empty string in Raycast CJS mode.
    #[tokio::test]
    async fn environment_support_path_is_non_empty() {
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  const hasPath = environment.supportPath && environment.supportPath.length > 0;
  return (
    <List>
      <List.Item title={hasPath ? "has-path" : "no-path"} />
    </List>
  );
}
"#;
        let ext = load_raycast("env-support-path", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "has-path");
    }

    /// `environment.assetsPath` is a non-empty string ending in "assets".
    #[tokio::test]
    async fn environment_assets_path_is_set() {
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  const p = environment.assetsPath;
  const ok = p && p.length > 0 && p.endsWith("assets") ? "ok" : "bad:" + p;
  return (
    <List>
      <List.Item title={ok} />
    </List>
  );
}
"#;
        let ext = load_raycast("env-assets-path", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok", "environment.assetsPath must end with 'assets'");
    }

    /// `LocalStorage.setItem` followed by `LocalStorage.getItem` returns the stored value.
    #[tokio::test]
    async fn local_storage_set_get_roundtrip() {
        clean_storage("local-storage-roundtrip");
        let tsx = r#"
import { List, LocalStorage } from "@raycast/api";
import { useEffect, useState } from "@raycast/api";

export default function Command() {
  const [val, setVal] = useState(null);
  useEffect(() => {
    LocalStorage.setItem("lskey", "lsvalue").then(() => {
      return LocalStorage.getItem("lskey");
    }).then((v) => {
      setVal(v);
    });
  }, []);

  return (
    <List>
      <List.Item title={val !== null && val !== undefined ? String(val) : "loading"} />
    </List>
  );
}
"#;
        let ext = load_raycast("local-storage-roundtrip", tsx).await;
        // First render: useEffect hasn't run yet, shows "loading"
        let _ = ext.on_search("").await;
        // Second render: effect has completed (Promise pump ran), shows stored value
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "lsvalue");
    }

    // ── Action.onAction callback routing (js-action) ──────────────────────────

    /// An `Action` with an `onAction` callback produces an action string prefixed
    /// with the extension name so it routes correctly through the Rust dispatcher.
    #[tokio::test]
    async fn action_with_on_action_produces_prefixed_action_string() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item
        title="My Item"
        actions={
          <ActionPanel>
            <Action title="Do Something" onAction={() => {}} />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("action-routing", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        // Action string must be "<ext-name>:js-action:<title>" so Rust routes it
        // to the right extension and the shim's onAction can find the handler.
        assert_eq!(results[0].action, "action-routing:js-action:Do Something");
    }

    /// Triggering a `js-action:<title>` via `on_action` calls the registered
    /// `onAction` callback, which can update state visible in the next search.
    #[tokio::test]
    async fn js_action_callback_is_invoked_and_updates_state() {
        let tsx = r#"
import { List, Action, ActionPanel, useState } from "@raycast/api";

export default function Command() {
  const [label, setLabel] = useState("before");
  return (
    <List>
      <List.Item
        title={label}
        actions={
          <ActionPanel>
            <Action title="Flip" onAction={() => setLabel("after")} />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("action-callback", tsx).await;

        // Initial render: state is "before"
        let r1 = ext.on_search("").await.expect("initial search");
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].title, "before");

        // Trigger the action: shim handles "js-action:Flip", calls setLabel("after")
        ext.on_action("js-action:Flip", None)
            .await
            .expect("on_action should not error");

        // Re-render: state updated to "after"
        let r2 = ext.on_search("").await.expect("post-action search");
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].title, "after");
    }

    // ── Action.Push / useNavigation ───────────────────────────────────────────

    /// `Action.Push` produces a `<ext-name>:js-push:<id>` action string.
    #[tokio::test]
    async fn action_push_produces_prefixed_push_action_string() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

function DetailView() {
  return <List><List.Item title="Detail" /></List>;
}

export default function Command() {
  return (
    <List>
      <List.Item
        title="Main"
        actions={
          <ActionPanel>
            <Action.Push title="Go to Detail" target={<DetailView />} />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("push-action-string", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        // Action must start with "<ext-name>:js-push:" followed by an ID
        assert!(
            results[0].action.starts_with("push-action-string:js-push:"),
            "expected action to start with 'push-action-string:js-push:', got: {}",
            results[0].action
        );
    }

    /// Triggering a `js-push:<id>` action pushes a new view onto the navigation
    /// stack; the next `on_search` renders the pushed component's items.
    #[tokio::test]
    async fn navigation_push_switches_to_detail_view() {
        let tsx = r#"
import { List, Action, ActionPanel } from "@raycast/api";

function DetailView() {
  return (
    <List>
      <List.Item title="Detail Item" />
    </List>
  );
}

export default function Command() {
  return (
    <List>
      <List.Item
        title="Main Item"
        actions={
          <ActionPanel>
            <Action.Push title="Open Detail" target={<DetailView />} />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("nav-push", tsx).await;

        // Step 1: initial render shows main view
        let r1 = ext.on_search("").await.expect("initial search");
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].title, "Main Item");
        let push_action = r1[0].action.clone(); // "nav-push:js-push:push_0"

        // Step 2: strip the extension prefix to get what the shim's onAction receives
        let shim_action = push_action
            .strip_prefix("nav-push:")
            .expect("action should have extension prefix");

        // Step 3: trigger navigation push
        ext.on_action(shim_action, None)
            .await
            .expect("on_action should not error");

        // Step 4: next search renders the pushed (detail) view
        let r2 = ext.on_search("").await.expect("post-push search");
        assert_eq!(r2.len(), 1, "pushed view should show exactly one item");
        assert_eq!(
            r2[0].title, "Detail Item",
            "pushed view should render DetailView"
        );
    }

    /// `useNavigation().pop()` returns to the previous view.
    #[tokio::test]
    async fn navigation_pop_returns_to_main_view() {
        let tsx = r#"
import { List, Action, ActionPanel, useNavigation } from "@raycast/api";

function DetailView() {
  const { pop } = useNavigation();
  return (
    <List>
      <List.Item
        title="Detail Item"
        actions={
          <ActionPanel>
            <Action title="Go Back" onAction={pop} />
          </ActionPanel>
        }
      />
    </List>
  );
}

export default function Command() {
  return (
    <List>
      <List.Item
        title="Main Item"
        actions={
          <ActionPanel>
            <Action.Push title="Open Detail" target={<DetailView />} />
          </ActionPanel>
        }
      />
    </List>
  );
}
"#;
        let ext = load_raycast("nav-pop", tsx).await;

        // Navigate to detail view
        let r1 = ext.on_search("").await.expect("initial search");
        let push_action = r1[0]
            .action
            .strip_prefix("nav-pop:")
            .expect("push action should have ext prefix")
            .to_string();
        ext.on_action(&push_action, None).await.expect("push");

        // Verify we're on the detail view
        let r2 = ext.on_search("").await.expect("pushed search");
        assert_eq!(r2[0].title, "Detail Item");
        let back_action = r2[0]
            .action
            .strip_prefix("nav-pop:")
            .expect("back action should have ext prefix")
            .to_string();

        // Pop back to main view
        ext.on_action(&back_action, None).await.expect("pop");
        let r3 = ext.on_search("").await.expect("post-pop search");
        assert_eq!(r3.len(), 1, "main view should show one item after pop");
        assert_eq!(r3[0].title, "Main Item", "should be back on main view");
    }

    /// `getSelectedText()` resolves to the OS selection (or "" if unavailable).
    #[tokio::test]
    async fn get_selected_text_is_exported_and_resolves() {
        let tsx = r#"
import { List, getSelectedText } from "@raycast/api";
import { useState, useEffect } from "@raycast/api";

export default function Command() {
  const [text, setText] = useState("loading");
  useEffect(() => {
    getSelectedText().then((t) => setText(t || "empty"));
  }, []);
  return (
    <List>
      <List.Item title={text} />
    </List>
  );
}
"#;
        let ext = load_raycast("get-selected-text", tsx).await;
        // First render (effect not yet run)
        let _ = ext.on_search("").await;
        // Second render (effect has run, Promise pump completed)
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        // The Promise must have resolved (state changed from initial "loading").
        // The actual value depends on the X11 primary selection in the test
        // environment, so we only assert it didn't stay at the initial value.
        assert_ne!(
            results[0].title, "loading",
            "getSelectedText() Promise should have resolved"
        );
    }

    // ── environment.theme ────────────────────────────────────────────────────

    /// `environment.theme` must be "dark" or "light" (never undefined, never
    /// crashing), even if `~/.pterry/settings.json` is absent.
    #[tokio::test]
    async fn environment_theme_is_dark_or_light() {
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  const t = environment.theme;
  const ok = (t === "dark" || t === "light") ? "ok" : "bad:" + t;
  return (
    <List>
      <List.Item title={ok} />
    </List>
  );
}
"#;
        let ext = load_raycast("env-theme-test", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok", "environment.theme must be 'dark' or 'light'");
    }

    // ── environment.extensionName / commandName / isDevelopment ─────────────

    /// `environment.extensionName` must match the name from `ExtensionMetadata`.
    #[tokio::test]
    async fn environment_extension_name_matches_metadata() {
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item title={environment.extensionName} />
    </List>
  );
}
"#;
        let ext = load_raycast("my-awesome-ext", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "my-awesome-ext");
    }

    /// `environment.commandName` must be the stem of `entry_point` (no extension).
    #[tokio::test]
    async fn environment_command_name_is_entry_point_stem() {
        // raycast_meta("search") sets entry_point to "search.tsx"; stem is "search"
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  return (
    <List>
      <List.Item title={environment.commandName} />
    </List>
  );
}
"#;
        let ext = load_raycast("search", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "search");
    }

    /// `environment.isDevelopment` must be false for extensions with
    /// `is_development: false` in their metadata.
    #[tokio::test]
    async fn environment_is_development_false_for_user_extensions() {
        let tsx = r#"
import { List, environment } from "@raycast/api";

export default function Command() {
  const v = environment.isDevelopment ? "dev" : "prod";
  return (
    <List>
      <List.Item title={v} />
    </List>
  );
}
"#;
        // raycast_meta sets is_development: false
        let ext = load_raycast("prod-ext", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "prod");
    }

    // ── getSelectedText native binding ───────────────────────────────────────

    /// `raycast.getSelectedText()` is a native binding that returns the OS text
    /// selection as a plain string (not a Promise). In test environments with no
    /// active X11 display or clipboard selection, it must return "" rather than panic.
    #[tokio::test]
    async fn native_get_selected_text_returns_string() {
        let js = r#"
function onSearch(query) {
    var sel = raycast.getSelectedText();
    // Must be a string (not undefined, not throwing)
    var result = (typeof sel === "string") ? "ok" : "not-string";
    raycast.updateList([{ title: result, action: "ok" }]);
}
"#;
        let meta = ExtensionMetadata {
            name: "native-get-selected-text".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::JavaScript,
            entry_point: "index.js".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(meta, js, false)
            .await
            .expect("JsExtension::new should not error");
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok");
    }

    // ── getPreferenceValues ───────────────────────────────────────────────────

    /// `getPreferenceValues()` reads from the extension's `preferences.json` file.
    /// Values stored in the file are returned as-is; missing keys return undefined.
    #[tokio::test]
    async fn get_preference_values_reads_from_preferences_file() {
        let name = "pref-values-test";
        let support_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".pterry")
            .join("extension-data")
            .join(name);
        let prefs_path = support_dir.join("preferences.json");
        std::fs::create_dir_all(&support_dir).expect("create support dir");
        std::fs::write(&prefs_path, r#"{"apiKey":"secret-key","limit":42}"#)
            .expect("write preferences.json");

        let tsx = r#"
import { List, getPreferenceValues } from "@raycast/api";

export default function Command() {
  const prefs = getPreferenceValues<{ apiKey: string }>();
  return (
    <List>
      <List.Item title={prefs.apiKey || "missing"} />
    </List>
  );
}
"#;
        let ext = load_raycast(name, tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 1, "should render one item");
        assert_eq!(
            results[0].title, "secret-key",
            "title should be the preference value"
        );

        let _ = std::fs::remove_file(&prefs_path);
    }

    /// `getPreferenceValues()` returns `{}` when no preferences file exists.
    #[tokio::test]
    async fn get_preference_values_returns_empty_when_no_file() {
        let name = "pref-values-no-file-test";
        // Ensure no preferences file exists
        let support_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".pterry")
            .join("extension-data")
            .join(name);
        let _ = std::fs::remove_file(support_dir.join("preferences.json"));

        let tsx = r#"
import { List, getPreferenceValues } from "@raycast/api";

export default function Command() {
  const prefs = getPreferenceValues<{ apiKey?: string }>();
  const title = prefs.apiKey !== undefined ? prefs.apiKey : "no-prefs";
  return (
    <List>
      <List.Item title={title} />
    </List>
  );
}
"#;
        let ext = load_raycast(name, tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "no-prefs");
    }

    // ── Grid component ────────────────────────────────────────────────────────

    /// A static `Grid` with one `Grid.Item` renders exactly one ExtensionItem.
    #[tokio::test]
    async fn grid_item_renders_as_extension_item() {
        let tsx = r#"
import { Grid, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <Grid>
      <Grid.Item
        title="My Image"
        subtitle="A caption"
        content="https://example.com/img.png"
        actions={
          <ActionPanel>
            <Action.OpenInBrowser url="https://example.com" />
          </ActionPanel>
        }
      />
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-item-basic", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 1, "should render exactly one item");
        assert_eq!(results[0].title, "My Image");
        assert_eq!(results[0].subtitle.as_deref(), Some("A caption"));
        // content maps to the icon field
        assert_eq!(
            results[0].icon.as_deref(),
            Some("https://example.com/img.png")
        );
        assert_eq!(results[0].action, "open-url:https://example.com");
    }

    /// Multiple `Grid.Item`s all appear in the result list.
    #[tokio::test]
    async fn grid_multiple_items_all_rendered() {
        let tsx = r#"
import { Grid, Action, ActionPanel } from "@raycast/api";

export default function Command() {
  return (
    <Grid>
      <Grid.Item title="First" content="img1.png" />
      <Grid.Item title="Second" content="img2.png" />
      <Grid.Item title="Third" content="img3.png" />
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-multiple-items", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].title, "First");
        assert_eq!(results[1].title, "Second");
        assert_eq!(results[2].title, "Third");
    }

    /// `Grid` with `onSearchTextChange` calls the callback on each search.
    #[tokio::test]
    async fn grid_on_search_text_change_fires() {
        let tsx = r#"
import { Grid, Action, ActionPanel, useState } from "@raycast/api";

export default function Command() {
  const [query, setQuery] = useState("");
  return (
    <Grid onSearchTextChange={setQuery} throttle>
      <Grid.Item title={query || "empty"} content="img.png" />
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-search-callback", tsx).await;
        let results = ext
            .on_search("hello")
            .await
            .expect("on_search should not error");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "hello");
    }

    /// `Grid.Section` groups items; section header sentinel appears before children.
    #[tokio::test]
    async fn grid_section_emits_header_sentinel() {
        let tsx = r#"
import { Grid } from "@raycast/api";

export default function Command() {
  return (
    <Grid>
      <Grid.Section title="My Section">
        <Grid.Item title="Item A" content="a.png" />
        <Grid.Item title="Item B" content="b.png" />
      </Grid.Section>
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-section", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        // Header sentinel + 2 items
        assert_eq!(results.len(), 3, "should have section header + 2 items");
        assert_eq!(results[0].action, "::section::");
        assert_eq!(results[0].title, "My Section");
        assert_eq!(results[1].title, "Item A");
        assert_eq!(results[2].title, "Item B");
    }

    /// Default `<Grid>` (no `itemSize`) stamps `gridColumns = 4` on items.
    #[tokio::test]
    async fn grid_stamps_default_columns_on_items() {
        let tsx = r#"
import { Grid } from "@raycast/api";
export default function Command() {
  return (
    <Grid>
      <Grid.Item title="A" content="a.png" />
      <Grid.Item title="B" content="b.png" />
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-cols-default", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].grid_columns,
            Some(4),
            "default itemSize maps to 4 columns"
        );
        assert_eq!(results[1].grid_columns, Some(4));
    }

    /// `<Grid itemSize="small">` stamps 5 columns; `large` stamps 3.
    #[tokio::test]
    async fn grid_stamps_columns_per_item_size() {
        let tsx = r#"
import { Grid } from "@raycast/api";
export default function Command() {
  return (
    <Grid itemSize="small">
      <Grid.Item title="X" content="x.png" />
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-cols-small", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        assert_eq!(
            results[0].grid_columns,
            Some(5),
            "small itemSize maps to 5 columns"
        );

        let tsx_large = r#"
import { Grid } from "@raycast/api";
export default function Command() {
  return (
    <Grid itemSize="large">
      <Grid.Item title="Y" content="y.png" />
    </Grid>
  );
}
"#;
        let ext2 = load_raycast("grid-cols-large", tsx_large).await;
        let results2 = ext2
            .on_search("")
            .await
            .expect("on_search should not error");
        assert_eq!(
            results2[0].grid_columns,
            Some(3),
            "large itemSize maps to 3 columns"
        );
    }

    /// Section header sentinels inside a Grid do NOT get `gridColumns` stamped.
    #[tokio::test]
    async fn grid_section_header_does_not_get_grid_columns() {
        let tsx = r#"
import { Grid } from "@raycast/api";
export default function Command() {
  return (
    <Grid>
      <Grid.Section title="Sec">
        <Grid.Item title="Item" content="i.png" />
      </Grid.Section>
    </Grid>
  );
}
"#;
        let ext = load_raycast("grid-section-no-cols", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");
        // results[0] is the section sentinel, results[1] is the item
        assert_eq!(results[0].action, "::section::");
        assert!(
            results[0].grid_columns.is_none(),
            "section headers must not carry gridColumns"
        );
        assert_eq!(
            results[1].grid_columns,
            Some(4),
            "items under the section get gridColumns"
        );
    }

    // ── MenuBarExtra ─────────────────────────────────────────────────────────

    /// MenuBarExtra items render as list items in the launcher (degraded mode).
    #[tokio::test]
    async fn menubarextra_items_render_as_list_items() {
        let tsx = r#"
import { MenuBarExtra } from "@raycast/api";

export default function Command() {
  return (
    <MenuBarExtra title="My Menu">
      <MenuBarExtra.Item title="Open Dashboard" />
      <MenuBarExtra.Item title="Refresh" />
    </MenuBarExtra>
  );
}
"#;
        let ext = load_raycast("menubarextra-basic", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        assert_eq!(results.len(), 2, "should render both MenuBarExtra items");
        assert_eq!(results[0].title, "Open Dashboard");
        assert_eq!(results[1].title, "Refresh");
    }

    /// MenuBarExtra.Section emits a section-header sentinel followed by its items.
    #[tokio::test]
    async fn menubarextra_section_emits_header_and_items() {
        let tsx = r#"
import { MenuBarExtra } from "@raycast/api";

export default function Command() {
  return (
    <MenuBarExtra title="My Menu">
      <MenuBarExtra.Section title="Actions">
        <MenuBarExtra.Item title="Alpha" />
        <MenuBarExtra.Item title="Beta" />
      </MenuBarExtra.Section>
    </MenuBarExtra>
  );
}
"#;
        let ext = load_raycast("menubarextra-section", tsx).await;
        let results = ext.on_search("").await.expect("on_search should not error");

        // Section header sentinel + 2 items
        assert_eq!(results.len(), 3, "should have section header + 2 items");
        assert_eq!(results[0].action, "::section::");
        assert_eq!(results[0].title, "Actions");
        assert_eq!(results[1].title, "Alpha");
        assert_eq!(results[2].title, "Beta");
    }
}
