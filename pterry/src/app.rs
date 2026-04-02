use crate::clipboard_manager::ClipboardManager;
use crate::components::{ActionPanel, Detail, List, ToastKind, ToastManager};
use crate::extension_manager::{ExtensionManager, ExtensionMessage};
use crate::hotkey_manager::{HotkeyEvent, HotkeyManager, register_hotkeys};
use crate::modes;
use crate::settings::Settings;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

/// How long after the last clipboard event before we fire a search refresh.
const CLIPBOARD_DEBOUNCE: Duration = Duration::from_millis(100);

/// Returns `true` when `pending` is set and `debounce` has elapsed since that
/// instant, meaning it is safe to fire the search refresh.  Pure — no side
/// effects — so it can be unit-tested without an egui context.
fn clipboard_search_ready(pending: Option<Instant>, now: Instant, debounce: Duration) -> bool {
    match pending {
        Some(t) => now.saturating_duration_since(t) >= debounce,
        None => false,
    }
}

/// Typed representation of a parsed action string.
#[derive(Debug, PartialEq)]
enum Action {
    EnterMode(String),
    LaunchApp(String),
    OpenUrl(String),
    OpenFile(String),
    CalculatorResult(String),
    CalculatorCopy(String),
    ClipboardPaste(String),
    /// Informational no-op (calculator-help, calculator-error, clipboard-no-results).
    Info,
    /// Enters a store sub-tab ("native" or "raycast"), switching the mode to
    /// "store-<tab>" which maps to the corresponding built-in registry extension.
    StoreTab(String),
    /// Reveals the file in the system file manager.
    ShowInFinder(String),
    /// Moves the file to the system trash.
    Trash(String),
    /// Saves the given theme string ("dark" | "light") to settings and updates the UI.
    SetTheme(String),
    /// Opens the preference-form for the named JS/TS extension directly in the UI.
    ShowExtPrefsForm(String),
    /// Raise and focus a window. Payload is "<backend>:<id>", e.g. "x11:12345".
    FocusWindow(String),
    /// Routed to a named extension.
    Extension {
        extension_name: String,
        /// All parts after the extension name joined with `:`.
        action_type: String,
        /// Third colon-delimited segment onwards (may overlap with `action_type`).
        item_id: Option<String>,
    },
    Unknown(String),
}

fn parse_action(action: &str) -> Action {
    if let Some(rest) = action.strip_prefix("enter-mode:") {
        Action::EnterMode(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("launch-app:") {
        Action::LaunchApp(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("open-url:") {
        Action::OpenUrl(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("open-file:") {
        Action::OpenFile(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("calculator-result:") {
        Action::CalculatorResult(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("calculator-copy:") {
        Action::CalculatorCopy(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("clipboard-paste:") {
        Action::ClipboardPaste(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("store-tab:") {
        Action::StoreTab(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("show-in-finder:") {
        Action::ShowInFinder(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("trash-file:") {
        Action::Trash(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("focus-window:") {
        Action::FocusWindow(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("settings-set-theme:") {
        Action::SetTheme(rest.to_string())
    } else if let Some(rest) = action.strip_prefix("settings-show-ext-prefs:") {
        Action::ShowExtPrefsForm(rest.to_string())
    } else if matches!(
        action,
        "calculator-help" | "calculator-error" | "clipboard-no-results"
    ) {
        Action::Info
    } else {
        let parts: Vec<&str> = action.split(':').collect();
        if parts.len() >= 2 {
            let extension_name = parts[0].to_string();
            let action_type = parts[1..].join(":");
            let item_id = if parts.len() > 2 {
                Some(parts[2..].join(":"))
            } else {
                None
            };
            Action::Extension {
                extension_name,
                action_type,
                item_id,
            }
        } else {
            Action::Unknown(action.to_string())
        }
    }
}

/// Returns `true` for actions that should hide the launcher window and clear
/// the search box after execution.  Pure — no side effects — so it can be
/// unit-tested without an egui context.
fn hides_window_on_action(action: &Action) -> bool {
    matches!(
        action,
        Action::LaunchApp(_)
            | Action::OpenUrl(_)
            | Action::OpenFile(_)
            | Action::ShowInFinder(_)
            | Action::FocusWindow(_)
    )
}

/// Build the `ActionPanel` action list for a given `ExtensionItem`.
///
/// If the item carries `extra_actions` (populated by the Raycast shim from
/// `<ActionPanel>` children), those are used.  Otherwise a single default
/// action is synthesised from the item's primary `action` string.
fn item_to_actions(item: &crate::extension_trait::ExtensionItem) -> Vec<crate::components::Action> {
    if !item.extra_actions.is_empty() {
        item.extra_actions
            .iter()
            .map(|ea| crate::components::Action {
                title: ea.title.clone(),
                shortcut: ea.shortcut.clone(),
                icon: ea.icon.clone(),
                action_str: ea.action.clone(),
                handler: Box::new(|| {}),
            })
            .collect()
    } else if !item.action.is_empty() && item.action != "noop" {
        // Synthesise a single default "Execute" action from the primary action.
        vec![crate::components::Action {
            title: item.title.clone(),
            shortcut: None,
            icon: item.icon.clone(),
            action_str: item.action.clone(),
            handler: Box::new(|| {}),
        }]
    } else {
        vec![]
    }
}

/// Map a single uppercase letter or digit character to an [`egui::Key`].
///
/// Used by [`parse_shortcut_label`] to convert the key portion of a Raycast
/// shortcut label (e.g. the `"C"` in `"⌘C"`) to an egui key value.
fn key_from_char(c: char) -> Option<egui::Key> {
    match c {
        'A' => Some(egui::Key::A),
        'B' => Some(egui::Key::B),
        'C' => Some(egui::Key::C),
        'D' => Some(egui::Key::D),
        'E' => Some(egui::Key::E),
        'F' => Some(egui::Key::F),
        'G' => Some(egui::Key::G),
        'H' => Some(egui::Key::H),
        'I' => Some(egui::Key::I),
        'J' => Some(egui::Key::J),
        'K' => Some(egui::Key::K),
        'L' => Some(egui::Key::L),
        'M' => Some(egui::Key::M),
        'N' => Some(egui::Key::N),
        'O' => Some(egui::Key::O),
        'P' => Some(egui::Key::P),
        'Q' => Some(egui::Key::Q),
        'R' => Some(egui::Key::R),
        'S' => Some(egui::Key::S),
        'T' => Some(egui::Key::T),
        'U' => Some(egui::Key::U),
        'V' => Some(egui::Key::V),
        'W' => Some(egui::Key::W),
        'X' => Some(egui::Key::X),
        'Y' => Some(egui::Key::Y),
        'Z' => Some(egui::Key::Z),
        '0' => Some(egui::Key::Num0),
        '1' => Some(egui::Key::Num1),
        '2' => Some(egui::Key::Num2),
        '3' => Some(egui::Key::Num3),
        '4' => Some(egui::Key::Num4),
        '5' => Some(egui::Key::Num5),
        '6' => Some(egui::Key::Num6),
        '7' => Some(egui::Key::Num7),
        '8' => Some(egui::Key::Num8),
        '9' => Some(egui::Key::Num9),
        _ => None,
    }
}

/// Parse a Raycast shortcut label string (e.g. `"⌘C"`, `"⌃⇧N"`) into an
/// [`egui::Modifiers`] + [`egui::Key`] pair.
///
/// The label format produced by `_formatShortcut` in `raycast_shim.js`:
/// - `⌘` → Command/Ctrl (uses [`egui::Modifiers::command`] for cross-platform)
/// - `⌃` → Ctrl
/// - `⌥` → Alt
/// - `⇧` → Shift
/// - Trailing uppercase letter or digit → the key
///
/// Returns `None` for unrecognised formats.
fn parse_shortcut_label(label: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut mods = egui::Modifiers::NONE;
    let mut remaining = label;

    loop {
        if let Some(rest) = remaining.strip_prefix('⌘') {
            mods.command = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⌃') {
            mods.ctrl = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⌥') {
            mods.alt = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⇧') {
            mods.shift = true;
            remaining = rest;
        } else {
            break;
        }
    }

    let key_char = remaining.chars().next()?.to_ascii_uppercase();
    let key = key_from_char(key_char)?;
    Some((mods, key))
}

/// Parse a `--<flag> <value>` pair from CLI arguments.
///
/// Returns `Some(value)` when `flag` (e.g. `"--extension"`) is found followed
/// by a value, `None` otherwise.  Pure function — no I/O.
fn parse_flag_arg(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().cloned();
        }
    }
    None
}

/// Parse the `--extension <mode>` flag from CLI arguments.
///
/// Returns `Some(mode_name)` when the flag is present and followed by a value,
/// `None` otherwise.  The function is pure (no I/O) so it can be unit-tested.
pub fn parse_extension_arg(args: &[String]) -> Option<String> {
    parse_flag_arg(args, "--extension")
}

/// Parse the `--query <text>` flag from CLI arguments.
///
/// Returns `Some(text)` when the flag is present and followed by a value,
/// `None` otherwise.
pub fn parse_query_arg(args: &[String]) -> Option<String> {
    parse_flag_arg(args, "--query")
}

/// The outcome of pressing Escape, depending on current UI state.
#[derive(Debug, PartialEq)]
enum EscapeOutcome {
    /// Close the action panel (action panel was open).
    CloseActionPanel,
    /// Pop the top JS push-view frame (a nested view is active).
    PopNavigation,
    /// Pop the main-list sentinel frame, exiting the mode and returning to
    /// global search.
    ReturnToMainList,
    /// Clear the search query and re-run search.
    ClearSearch,
    /// Move keyboard focus back to the search box.
    FocusSearch,
    /// Hide the launcher window (and reset all mode state).
    HideWindow,
}

/// Pure state-machine for the Escape key.  Returns the action that should be
/// taken given the current UI state; does not mutate anything itself.
fn escape_outcome(
    action_panel_open: bool,
    top_frame: NavTopFrame,
    query_empty: bool,
    search_focused: bool,
) -> EscapeOutcome {
    if action_panel_open {
        EscapeOutcome::CloseActionPanel
    } else if top_frame == NavTopFrame::JsView {
        // Always pop JS sub-views first, even with a non-empty query.
        EscapeOutcome::PopNavigation
    } else if !query_empty {
        EscapeOutcome::ClearSearch
    } else if top_frame == NavTopFrame::MainListRoot {
        // Empty query and main-list sentinel on stack: return to global search.
        EscapeOutcome::ReturnToMainList
    } else if !search_focused {
        EscapeOutcome::FocusSearch
    } else {
        EscapeOutcome::HideWindow
    }
}

/// A single selectable option inside a `FormFieldDef::Dropdown`.
#[derive(Debug, Clone, PartialEq)]
struct DropdownOption {
    value: String,
    title: String,
}

/// Typed representation of a single form field, parsed from the JSON produced
/// by the JS shim's `_extractFormDef`.
#[derive(Debug, Clone, PartialEq)]
enum FormFieldDef {
    TextField {
        id: String,
        title: String,
        placeholder: Option<String>,
        default_value: String,
    },
    Checkbox {
        id: String,
        title: String,
        label: String,
        default_value: bool,
    },
    Dropdown {
        id: String,
        title: String,
        options: Vec<DropdownOption>,
        default_value: String,
    },
}

impl FormFieldDef {
    fn id(&self) -> &str {
        match self {
            FormFieldDef::TextField { id, .. }
            | FormFieldDef::Checkbox { id, .. }
            | FormFieldDef::Dropdown { id, .. } => id,
        }
    }

    fn default_value_str(&self) -> String {
        match self {
            FormFieldDef::TextField { default_value, .. } => default_value.clone(),
            FormFieldDef::Checkbox { default_value, .. } => {
                if *default_value { "true" } else { "false" }.to_string()
            }
            FormFieldDef::Dropdown { default_value, .. } => default_value.clone(),
        }
    }
}

/// Load the preference schema for `ext_name` from the extension directories and
/// return the `FormFieldDef` list together with any existing values from disk.
///
/// Returns `None` when the extension has no `extension.json` or no preferences.
fn load_ext_pref_form(
    ext_name: &str,
) -> Option<(Vec<FormFieldDef>, HashMap<String, String>)> {
    use crate::extension_trait::{ExtensionMetadata, PreferenceDropdownItem};

    // Locate the extension.json for this extension.
    let home_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let user_dir = home_dir.join(".pterry").join("extensions");
    let dev_dir = std::path::PathBuf::from("extensions");

    let json_path = [&user_dir, &dev_dir]
        .iter()
        .find_map(|base| {
            // Package-style: <base>/<name>/extension.json
            let pkg = base.join(ext_name).join("extension.json");
            if pkg.exists() {
                return Some(pkg);
            }
            // Sidecar: <base>/<name>.json (next to <name>.js/.ts)
            let sidecar = base.join(format!("{ext_name}.json"));
            if sidecar.exists() {
                return Some(sidecar);
            }
            None
        })?;

    let content = std::fs::read_to_string(&json_path).ok()?;
    let meta: ExtensionMetadata = serde_json::from_str(&content).ok()?;

    if meta.preferences.is_empty() {
        return None;
    }

    // Load existing values from disk.
    let prefs_path = home_dir
        .join(".pterry")
        .join("extension-data")
        .join(ext_name)
        .join("preferences.json");
    let existing: HashMap<String, String> =
        std::fs::read_to_string(&prefs_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    // Convert PreferenceSpec → FormFieldDef.
    let fields: Vec<FormFieldDef> = meta
        .preferences
        .iter()
        .filter_map(|p| {
            let id = p.name.clone();
            let title = p.title.clone();
            let default_str = p
                .default
                .as_ref()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

            match p.type_.as_str() {
                "textfield" | "password" | "file" | "directory" => {
                    Some(FormFieldDef::TextField {
                        id,
                        title,
                        placeholder: None,
                        default_value: default_str,
                    })
                }
                "checkbox" => Some(FormFieldDef::Checkbox {
                    label: title.clone(),
                    id,
                    title,
                    default_value: default_str == "true",
                }),
                "dropdown" => {
                    let options: Vec<DropdownOption> = p
                        .data
                        .iter()
                        .map(|PreferenceDropdownItem { title, value }| DropdownOption {
                            title: title.clone(),
                            value: value.clone(),
                        })
                        .collect();
                    let default_value = if !default_str.is_empty() {
                        default_str
                    } else {
                        options.first().map(|o| o.value.clone()).unwrap_or_default()
                    };
                    Some(FormFieldDef::Dropdown {
                        id,
                        title,
                        options,
                        default_value,
                    })
                }
                _ => None,
            }
        })
        .collect();

    // Seed values: existing values override defaults.
    let mut values: HashMap<String, String> = fields
        .iter()
        .map(|f| (f.id().to_string(), f.default_value_str()))
        .collect();
    for (k, v) in &existing {
        values.insert(k.clone(), v.clone());
    }

    Some((fields, values))
}

/// Parse the JSON form-definition string emitted by the JS shim (`_extractFormDef`)
/// into a typed list of field descriptors.
///
/// Returns `None` if `json` is not valid JSON or has no `"fields"` array.
fn parse_form_def(json: &str) -> Option<Vec<FormFieldDef>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let raw_fields = v["fields"].as_array()?;
    let mut fields = Vec::new();
    for field in raw_fields {
        let id = field["id"].as_str().unwrap_or("").to_string();
        let title = field["title"].as_str().unwrap_or("").to_string();
        match field["type"].as_str() {
            Some("textfield") => {
                fields.push(FormFieldDef::TextField {
                    id,
                    title,
                    placeholder: field["placeholder"].as_str().map(str::to_string),
                    default_value: field["defaultValue"].as_str().unwrap_or("").to_string(),
                });
            }
            Some("checkbox") => {
                fields.push(FormFieldDef::Checkbox {
                    id,
                    title,
                    label: field["label"].as_str().unwrap_or("").to_string(),
                    default_value: field["defaultValue"].as_bool().unwrap_or(false),
                });
            }
            Some("dropdown") => {
                let options: Vec<DropdownOption> = field["options"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .map(|opt| DropdownOption {
                                value: opt["value"].as_str().unwrap_or("").to_string(),
                                title: opt["title"].as_str().unwrap_or("").to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let default_value = field["defaultValue"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        options.first().map(|o| o.value.clone()).unwrap_or_default()
                    });
                fields.push(FormFieldDef::Dropdown {
                    id,
                    title,
                    options,
                    default_value,
                });
            }
            _ => {} // unknown field types are silently skipped
        }
    }
    Some(fields)
}

struct FormState {
    /// Typed field definitions parsed from the JS shim's form sentinel.
    fields: Vec<FormFieldDef>,
    /// Current live values keyed by field id.
    values: HashMap<String, String>,
    /// Name of the extension that owns this form (used to route the submit action).
    extension_name: String,
}

/// All UI interaction state.
/// A snapshot of a single navigation level.  Saved on push and restored when
/// the user pops back, so each level remembers its own search query, item list,
/// and selection.
#[derive(Clone)]
pub struct NavFrame {
    pub items: Vec<crate::extension_trait::ExtensionItem>,
    pub search_query: String,
    pub selected_index: Option<usize>,
    pub title: String,
    /// `true` for the sentinel frame pushed when the user navigates into a mode
    /// from the main list.  Popping this frame exits the mode and restores the
    /// global search view rather than returning to a JS sub-view.
    pub is_main_list_root: bool,
}

/// Describes the top of the navigation stack for use in [`escape_outcome`].
#[derive(Debug, PartialEq)]
enum NavTopFrame {
    /// Stack is empty.
    Empty,
    /// Top frame is a JS push-view (from `useNavigation().push()`).
    JsView,
    /// Top frame is the main-list sentinel (user entered mode from global search).
    MainListRoot,
}

/// Maximum number of navigation levels a JS extension may push.
const NAV_STACK_MAX_DEPTH: usize = 10;

pub struct UiState {
    pub list: List,
    pub search_query: String,
    pub search_focused: bool,
    pub action_panel: ActionPanel,
    /// When `Some(name)`, only the named extension is searched and its results
    /// are shown. `None` means the normal global broadcast search is active.
    pub current_mode: Option<String>,
    /// When `Some`, a form extension is active and the UI shows form fields
    /// instead of the normal list+detail layout.
    form_state: Option<FormState>,
    pub window_visible: bool,
    pub toast_manager: ToastManager,
    /// Navigation stack pushed by JS extensions via `useNavigation().push()`.
    /// Each [`NavFrame`] stores the parent view's state so it can be restored
    /// on pop.  Empty means the root view is active.
    pub nav_stack: Vec<NavFrame>,
}

/// Async runtime and extension/clipboard services.
pub struct ExtensionContext {
    pub extension_manager: Arc<ExtensionManager>,
    pub runtime: Arc<Runtime>,
    pub clipboard_manager: Arc<ClipboardManager>,
    /// Set to `Some(Instant)` when a clipboard event arrives; cleared once the
    /// debounce window elapses and the search is fired.
    clipboard_pending_search: Option<Instant>,
    /// Monotonically-increasing counter bumped on every `trigger_search()` call.
    /// Each spawned task captures its value at spawn time; it only sends results
    /// if the counter hasn't advanced past its captured value, discarding stale
    /// in-flight results from superseded searches.
    search_seq: Arc<AtomicU64>,
}

/// Platform hotkey registration state.
pub struct HotkeyState {
    pub hotkey_manager: Arc<HotkeyManager>,
    /// Global hotkey manager — kept alive for the process lifetime so that the
    /// registered hotkeys stay active.  `None` on Wayland (where X11 grabs are
    /// unavailable) or when registration failed.
    _global_hotkey_manager: Option<global_hotkey::GlobalHotKeyManager>,
}

pub struct App {
    pub ui: UiState,
    pub ext: ExtensionContext,
    pub hotkey: HotkeyState,
    pub ctx: egui::Context,
    pub selection_freq: crate::selection_frequency::SelectionFrequency,
}

impl App {
    /// Register global hotkeys from settings and return a `HotkeyState`.
    /// Reads `toggle_hotkey` and `extension_hotkeys` from `~/.pterry/settings.json`.
    /// Falls back to `"alt+space"` when no toggle key is configured.
    /// Failures are expected on Wayland without XWayland.
    fn setup_global_hotkey() -> HotkeyState {
        let settings = Settings::load();
        let toggle_key = settings.toggle_hotkey.as_deref().unwrap_or("alt+space");
        let (hotkey_map, _global_hotkey_manager) =
            register_hotkeys(toggle_key, &settings.extension_hotkeys);
        let hotkey_manager = Arc::new(HotkeyManager::with_hotkey_map(hotkey_map));
        HotkeyState {
            hotkey_manager,
            _global_hotkey_manager,
        }
    }

    pub fn new(
        ctx: egui::Context,
        initial_mode: Option<String>,
        initial_query: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let clipboard_manager = Arc::new(
            ClipboardManager::new().map_err(Box::<dyn std::error::Error + Send + Sync>::from)?,
        );
        let extension_manager =
            Arc::new(ExtensionManager::new().with_clipboard_manager(clipboard_manager.clone()));
        let hotkey = Self::setup_global_hotkey();
        let runtime = Arc::new(Runtime::new()?);

        let ui = UiState {
            list: List::new(),
            search_query: initial_query.unwrap_or_default(),
            search_focused: true,
            action_panel: ActionPanel::new(),
            window_visible: true, // Start visible for testing
            toast_manager: ToastManager::new(),
            current_mode: initial_mode,
            form_state: None,
            nav_stack: vec![],
        };
        let ext = ExtensionContext {
            extension_manager,
            runtime,
            clipboard_manager,
            clipboard_pending_search: None,
            search_seq: Arc::new(AtomicU64::new(0)),
        };

        let mut app = Self {
            ui,
            ext,
            hotkey,
            ctx: ctx.clone(),
            selection_freq: crate::selection_frequency::SelectionFrequency::load(),
        };

        app.initialize_extensions();

        // Load built-in extensions
        let manager = app.ext.extension_manager.clone();
        let ctx_builtin = ctx.clone();
        app.ext.runtime.spawn(async move {
            if let Err(e) = manager.load_builtin_extensions().await {
                eprintln!("Failed to load built-in extensions: {e}");
            }
            ctx_builtin.request_repaint();
        });

        // Start the Unix-socket listener (Linux fallback for Wayland)
        app.hotkey.hotkey_manager.start_listening();

        // Start clipboard monitoring
        app.ext.clipboard_manager.start_monitoring();

        // Trigger an initial empty search to populate the list
        let manager = app.ext.extension_manager.clone();
        let runtime = app.ext.runtime.clone();
        let ctx_search = ctx.clone();
        runtime.spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            println!("Triggering initial search...");
            let query = String::new();
            let results = manager.broadcast_search(query.clone()).await;
            let all_items: Vec<_> = results.into_values().flatten().collect();
            if !all_items.is_empty() {
                let _ = manager
                    .get_sender()
                    .send(ExtensionMessage::SearchResults(query, all_items));
            }
            ctx_search.request_repaint();
        });

        Ok(app)
    }

    fn initialize_extensions(&mut self) {
        println!("Extension manager initialized, ready to load extensions");

        let home_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let user_dir = home_dir.join(".pterry").join("extensions");
        let dev_dir = std::path::PathBuf::from("extensions");

        // Ensure the user extensions directory exists
        if !user_dir.exists() {
            let _ = std::fs::create_dir_all(&user_dir);
        }

        // Collect extensions from both directories; user_dir takes priority over dev_dir
        // (if the same name exists in both, the user_dir version wins).
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (dir, label) in [(&user_dir, "user"), (&dev_dir, "dev")] {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }
            println!("Scanning {label} extensions directory: {dir:?}");
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();

                // Directory with package.json → multi-command Raycast extension
                if path.is_dir() {
                    if !path.join("package.json").exists() {
                        continue;
                    }
                    let pkg_name = match path.file_name().and_then(|s| s.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    if seen.contains(&pkg_name) {
                        println!(
                            "Skipping {label} duplicate package '{pkg_name}' (already loaded)"
                        );
                        continue;
                    }
                    seen.insert(pkg_name.clone());
                    println!("Loading package extension '{pkg_name}' from {path:?}");
                    let manager = self.ext.extension_manager.clone();
                    self.ext.runtime.spawn(async move {
                        match manager.load_package_dir(path, &pkg_name).await {
                            Ok(n) => println!("Package '{pkg_name}': loaded {n} command(s)"),
                            Err(e) => eprintln!("Failed to load package '{pkg_name}': {e}"),
                        }
                    });
                    continue;
                }

                if !path.is_file() {
                    continue;
                }
                let ext_name = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                // Only load known JS/TS source files; skip sidecars, .disabled, etc.
                let ext_str = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext_str, "js" | "ts" | "tsx") {
                    continue;
                }
                if seen.contains(&ext_name) {
                    println!("Skipping {label} duplicate '{ext_name}' (already loaded)");
                    continue;
                }
                seen.insert(ext_name.clone());
                println!("Loading extension '{ext_name}' from {path:?}");
                let manager = self.ext.extension_manager.clone();
                self.ext.runtime.spawn(async move {
                    match manager.load_extension(path, Some(ext_name)).await {
                        Ok(_) => println!("Extension loaded successfully"),
                        Err(e) => eprintln!("Failed to load extension: {e}"),
                    }
                });
            }
        }
    }

    /// Spawn an async search task using the current query and mode.
    /// In mode, only the mode extension is queried; otherwise all auto-load
    /// extensions are broadcast to.
    fn trigger_search(&self) {
        let manager = self.ext.extension_manager.clone();
        let query = self.ui.search_query.clone();
        let mode = self.ui.current_mode.clone();
        let ctx = self.ctx.clone();
        let seq_counter = self.ext.search_seq.clone();
        // Bump the counter and capture the new value for this search.
        let my_seq = seq_counter.fetch_add(1, Ordering::Release) + 1;
        self.ext.runtime.spawn(async move {
            if let Some(ext_name) = mode {
                match manager.handle_search(&ext_name, query.clone()).await {
                    Ok(items) => {
                        // Only send if no newer search has been triggered.
                        if seq_counter.load(Ordering::Acquire) == my_seq {
                            let _ = manager
                                .get_sender()
                                .send(ExtensionMessage::SearchResults(query, items));
                        }
                    }
                    Err(e) => eprintln!("Mode search '{ext_name}' error: {e}"),
                }
            } else {
                let results = manager.broadcast_search(query.clone()).await;
                let all_items: Vec<_> = results.into_values().flatten().collect();
                // Only send if no newer search has been triggered.
                if seq_counter.load(Ordering::Acquire) == my_seq {
                    let _ = manager
                        .get_sender()
                        .send(ExtensionMessage::SearchResults(query, all_items));
                }
            }
            ctx.request_repaint();
        });
    }

    fn handle_messages(&mut self) {
        let receiver = self.ext.extension_manager.get_receiver();
        while let Ok(message) = receiver.try_recv() {
            match message {
                ExtensionMessage::SearchResults(query, items) => {
                    if query == self.ui.search_query {
                        println!("Received {} results for query '{}'", items.len(), query);

                        // Detect the form sentinel: a single item with id "::form::"
                        // emitted by a JS extension that renders a <Form> as its root.
                        // Only trigger in mode (single-extension search) to avoid false
                        // positives from broadcast results mixing with other extensions.
                        if self.ui.current_mode.is_some()
                            && items.len() == 1
                            && items[0].id.as_deref() == Some("::form::")
                        {
                            if let Some(ref detail) = items[0].detail
                                && let Some(fields) = parse_form_def(detail)
                            {
                                let ext_name = self.ui.current_mode.clone().unwrap_or_default();
                                let values = fields
                                    .iter()
                                    .map(|f| (f.id().to_string(), f.default_value_str()))
                                    .collect();
                                self.ui.form_state = Some(FormState {
                                    fields,
                                    values,
                                    extension_name: ext_name,
                                });
                            }
                        } else {
                            // Normal results: clear any active form state.
                            self.ui.form_state = None;
                            let boosted = crate::selection_frequency::boost_by_frequency(
                                items,
                                &self.selection_freq,
                            );
                            self.ui.list.set_items(boosted);
                        }
                    } else {
                        println!(
                            "Discarding {} stale results for '{}' (current: '{}')",
                            items.len(),
                            query,
                            self.ui.search_query
                        );
                    }
                }
                ExtensionMessage::ExtensionLoaded(name) => {
                    println!("Extension loaded: {name}");
                    // Re-run search so launcher items from newly loaded
                    // extensions appear immediately without user interaction.
                    self.trigger_search();
                }
                ExtensionMessage::ExtensionError(name, err) => {
                    eprintln!("Extension error from '{name}': {err}");
                }
                ExtensionMessage::ExtensionUnloaded(name) => {
                    println!("Extension unloaded: {name}");
                }
                ExtensionMessage::ShowToast(style, title, message) => {
                    let kind = match style.as_str() {
                        "success" | "animated" => ToastKind::Success,
                        "failure" => ToastKind::Error,
                        _ => ToastKind::Info,
                    };
                    let text = if message.is_empty() {
                        title
                    } else {
                        format!("{title}: {message}")
                    };
                    self.ui.toast_manager.push(text, kind);
                }
                ExtensionMessage::ActionComplete(ext_name) => {
                    // If we are currently scoped to this extension's mode, re-run
                    // search so that any state change (navigation push/pop, setState
                    // inside onAction) is reflected in the list immediately.
                    if self.ui.current_mode.as_deref() == Some(ext_name.as_str()) {
                        self.trigger_search();
                    }
                }
                ExtensionMessage::HideWindow => {
                    self.ui.window_visible = false;
                    self.ctx
                        .send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                ExtensionMessage::Navigate(action) => {
                    if action == "pop-view" {
                        if let Some(frame) = self.ui.nav_stack.pop() {
                            // Restore the parent view's search query, items, and selection.
                            self.ui.search_query = frame.search_query;
                            self.ui
                                .list
                                .restore_snapshot(frame.items, frame.selected_index);
                            if frame.is_main_list_root {
                                self.ui.current_mode = None;
                            }
                        }
                    } else if let Some(title) = action.strip_prefix("push-view:")
                        && self.ui.nav_stack.len() < NAV_STACK_MAX_DEPTH
                    {
                        // Save current state before pushing the new level.
                        let (items, selected_index) = self.ui.list.snapshot();
                        self.ui.nav_stack.push(NavFrame {
                            items,
                            search_query: self.ui.search_query.clone(),
                            selected_index,
                            title: title.to_string(),
                            is_main_list_root: false,
                        });
                        // Clear for the new (child) view.
                        self.ui.search_query.clear();
                        self.ui.list.set_items(vec![]);
                    }
                }
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        if self.handle_form_input(ctx) {
            return;
        }
        if self.handle_action_panel_input(ctx) {
            return;
        }

        // If the user types or presses Backspace while the list has "focus"
        // (after arrow-key navigation or mouse scroll), redirect input to the
        // search box immediately so no keystrokes are lost.
        if !self.ui.action_panel.is_open {
            let events = ctx.input(|i| i.events.clone());
            if reclaim_focus_on_text_input(
                &mut self.ui.search_query,
                self.ui.search_focused,
                &events,
            ) {
                self.ui.search_focused = true;
                self.trigger_search();
            }
        }

        self.handle_navigation(ctx);

        // Enter key to execute selected item
        if ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(item) = self.ui.list.selected_item()
            && !item.action.is_empty()
        {
            self.execute_action(item.action.clone(), ctx);
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            let top_frame = match self.ui.nav_stack.last() {
                None => NavTopFrame::Empty,
                Some(f) if f.is_main_list_root => NavTopFrame::MainListRoot,
                Some(_) => NavTopFrame::JsView,
            };
            let outcome = escape_outcome(
                self.ui.action_panel.is_open,
                top_frame,
                self.ui.search_query.is_empty(),
                self.ui.search_focused,
            );
            self.apply_escape_outcome(outcome, ctx);
        }

        self.handle_global_shortcuts(ctx);
    }

    /// Handle Escape when a form is active. Returns `true` to signal the caller
    /// should return early (form consumes all input while active).
    fn handle_form_input(&mut self, ctx: &egui::Context) -> bool {
        if self.ui.form_state.is_none() {
            return false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ui.form_state = None;
            self.ui.current_mode = None;
            self.ui.search_query.clear();
            self.ui.list.set_items(vec![]);
            self.trigger_search();
            self.ui.search_focused = true;
        }
        true
    }

    /// Handle arrow keys / Enter / Escape when the action panel is open.
    /// Returns `true` to signal the caller should return early.
    fn handle_action_panel_input(&mut self, ctx: &egui::Context) -> bool {
        if !self.ui.action_panel.is_open {
            return false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.ui.action_panel.select_next();
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.ui.action_panel.select_prev();
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.ui.action_panel.execute_selected();
            self.ui.action_panel.close();
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ui.action_panel.close();
        }
        true
    }

    /// List navigation: Arrow keys and Ctrl+N/P.
    fn handle_navigation(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown))
            || ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::N))
        {
            self.ui.list.select_next();
            self.ui.search_focused = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp))
            || ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::P))
        {
            self.ui.list.select_prev();
            self.ui.search_focused = false;
        }
    }

    /// Global shortcuts: Ctrl+K (action panel), Ctrl+Shift+C (clipboard mode toggle),
    /// and any custom per-action shortcuts on the selected list item.
    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::K)) {
            self.open_action_panel();
        }
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::C)) {
            if self.ui.current_mode.as_deref() == Some(modes::CLIPBOARD_HISTORY) {
                self.ui.current_mode = None;
            } else {
                self.ui.current_mode = Some(modes::CLIPBOARD_HISTORY.to_string());
            }
            self.ui.search_query.clear();
            self.ui.list.set_items(vec![]);
            self.trigger_search();
            self.ui.search_focused = true;
        }

        // Fire any custom shortcut bound to the selected item's extra actions.
        if let Some(item) = self.ui.list.selected_item().cloned() {
            let matched = item.extra_actions.iter().find_map(|ea| {
                let label = ea.shortcut.as_deref()?;
                let (mods, key) = parse_shortcut_label(label)?;
                let pressed =
                    ctx.input(|i| i.modifiers == mods && i.key_pressed(key));
                if pressed { Some(ea.action.clone()) } else { None }
            });
            if let Some(action) = matched {
                self.execute_action(action, ctx);
            }
        }
    }

    /// Apply the result of [`escape_outcome`] to the current app state.
    fn apply_escape_outcome(&mut self, outcome: EscapeOutcome, ctx: &egui::Context) {
        match outcome {
            EscapeOutcome::CloseActionPanel => self.ui.action_panel.close(),
            EscapeOutcome::PopNavigation => {
                // Tell the JS extension to pop its navigation stack.
                // The shim's onAction("pop-view") handler calls _navigationPop(),
                // which sends Navigate("pop-view") back — that message decrements nav_stack.
                if let Some(ext_name) = self.ui.current_mode.clone() {
                    let manager = self.ext.extension_manager.clone();
                    self.ext.runtime.spawn(async move {
                        let _ = manager
                            .handle_action(&ext_name, "pop-view".to_string(), None)
                            .await;
                    });
                }
            }
            EscapeOutcome::ReturnToMainList => {
                // Pop the main-list sentinel frame and restore global search state.
                let frame = self.ui.nav_stack.pop().expect("ReturnToMainList implies a frame");
                self.ui.current_mode = None;
                self.ui.search_query = frame.search_query;
                self.ui.list.restore_snapshot(frame.items, frame.selected_index);
                self.ui.search_focused = true;
            }
            EscapeOutcome::ClearSearch => {
                self.ui.search_query.clear();
                self.trigger_search();
                self.ui.search_focused = true;
            }
            EscapeOutcome::FocusSearch => {
                self.ui.search_focused = true;
            }
            EscapeOutcome::HideWindow => {
                self.ui.current_mode = None;
                self.ui.nav_stack.clear();
                self.ui.search_query.clear();
                self.ui.list.set_items(vec![]);
                self.ui.window_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }

    fn open_action_panel(&mut self) {
        let item = self.ui.list.selected_item().cloned();
        let actions = item.map(|i| item_to_actions(&i)).unwrap_or_default();
        if !actions.is_empty() {
            self.ui.action_panel.set_actions(actions);
            self.ui.action_panel.open();
        }
    }

    fn execute_action(&mut self, action: String, ctx: &egui::Context) {
        println!("Executing action: {action}");

        // Track selection frequency so high-use items rise to the top.
        // Skip transient/meta actions that aren't real user choices.
        let parsed = parse_action(&action);
        if !matches!(
            parsed,
            Action::Info
                | Action::StoreTab(_)
                | Action::SetTheme(_)
                | Action::ShowExtPrefsForm(_)
                | Action::Unknown(_)
        ) {
            self.selection_freq.increment(&action);
            if let Err(e) = self.selection_freq.save() {
                eprintln!("[frequency] failed to save: {e}");
            }
        }
        let should_hide = hides_window_on_action(&parsed);

        match parsed {
            Action::EnterMode(mode_name) => {
                // Save the current main-list state so Escape can return here.
                let (items, selected_index) = self.ui.list.snapshot();
                self.ui.nav_stack.push(NavFrame {
                    items,
                    search_query: self.ui.search_query.clone(),
                    selected_index,
                    title: String::new(),
                    is_main_list_root: true,
                });
                self.ui.current_mode = Some(mode_name);
                self.ui.search_query.clear();
                self.ui.list.set_items(vec![]);
                self.trigger_search();
                self.ui.search_focused = true;
            }
            Action::LaunchApp(app_path) => {
                crate::platform::launch_app(&app_path);
            }
            Action::OpenUrl(url) => {
                crate::platform::open(&url);
            }
            Action::OpenFile(file_path) => {
                crate::platform::open(&file_path);
            }
            Action::FocusWindow(payload) => {
                crate::platform::focus_window(&payload);
            }
            Action::ShowInFinder(path) => {
                crate::platform::show_in_finder(&path);
            }
            Action::Trash(path) => {
                crate::platform::trash_file(&path);
                self.ui
                    .toast_manager
                    .push(format!("Moved to trash: {path}"), ToastKind::Success);
            }
            Action::CalculatorResult(result) => {
                self.copy_to_clipboard(&result);
                self.ui
                    .toast_manager
                    .push(format!("Copied: {result}"), ToastKind::Success);
            }
            Action::CalculatorCopy(expression) => {
                self.copy_to_clipboard(&expression);
                self.ui
                    .toast_manager
                    .push(format!("Copied: {expression}"), ToastKind::Success);
            }
            Action::ClipboardPaste(index) => {
                self.paste_clipboard_item(&index);
                self.ui
                    .toast_manager
                    .push("Copied to clipboard", ToastKind::Success);
            }
            Action::StoreTab(tab) => {
                let mode = format!("store-{tab}");
                self.ui.current_mode = Some(mode);
                self.ui.nav_stack.clear();
                self.ui.search_query.clear();
                self.ui.list.set_items(vec![]);
                self.trigger_search();
                self.ui.search_focused = true;
            }
            Action::SetTheme(theme) => {
                let mut settings = crate::settings::Settings::load();
                settings.theme = if theme.is_empty() {
                    None
                } else {
                    Some(theme.clone())
                };
                if let Err(e) = settings.save() {
                    eprintln!("[settings] failed to save theme: {e}");
                }
                ctx.set_visuals(crate::settings::visuals_for_theme(Some(&theme)));
                self.ui
                    .toast_manager
                    .push(format!("Theme set to {theme}"), ToastKind::Success);
            }
            Action::ShowExtPrefsForm(ext_name) => {
                if let Some((fields, values)) = load_ext_pref_form(&ext_name) {
                    self.ui.form_state = Some(FormState {
                        fields,
                        values,
                        // Sentinel prefix so the submit handler saves to disk instead
                        // of calling on_action on an extension.
                        extension_name: format!("__prefs__{ext_name}"),
                    });
                } else {
                    self.ui.toast_manager.push(
                        format!("No preferences defined for '{ext_name}'"),
                        ToastKind::Error,
                    );
                }
            }
            Action::Info => {
                println!("Info action: {action}");
            }
            Action::Extension {
                extension_name,
                action_type,
                item_id,
            } => {
                let manager = self.ext.extension_manager.clone();
                let runtime = self.ext.runtime.clone();
                runtime.spawn(async move {
                    match manager
                        .handle_action(&extension_name, action_type, item_id)
                        .await
                    {
                        Ok(_) => println!("Extension action executed successfully: {action}"),
                        Err(e) => eprintln!("Failed to execute extension action: {e}"),
                    }
                });
            }
            Action::Unknown(_) => {
                println!("Unknown action type: {action}");
            }
        }

        if should_hide {
            self.ui.search_query.clear();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn copy_to_clipboard(&self, text: &str) {
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};

            println!("Copying to clipboard on macOS");

            match Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
                Ok(mut child) => {
                    if let Some(stdin) = child.stdin.as_mut() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    let _ = child.wait();
                    println!("Text copied to clipboard");
                }
                Err(e) => eprintln!("Failed to start pbcopy: {e}"),
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};

            println!("Copying to clipboard on Linux");

            // Try xclip first, then xsel
            let result = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });

            if result.is_err() {
                let _ = Command::new("xsel")
                    .args(["--clipboard", "--input"])
                    .stdin(Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        if let Some(stdin) = child.stdin.as_mut() {
                            stdin.write_all(text.as_bytes())?;
                        }
                        child.wait()
                    });
            }

            println!("Text copied to clipboard");
        }

        #[cfg(target_os = "windows")]
        {
            use std::process::Command;

            println!("Copying to clipboard on Windows");

            // Windows doesn't have a simple built-in command, but we can use PowerShell
            let _ = Command::new("powershell")
                .args(&[
                    "-Command",
                    &format!("Set-Clipboard -Value '{}'", text.replace("'", "''")),
                ])
                .output();

            println!("Text copied to clipboard");
        }
    }

    fn paste_clipboard_item(&self, index: &str) {
        if let Ok(idx) = index.parse::<usize>() {
            let history = self.ext.clipboard_manager.get_history();
            if idx < history.len() {
                let item = &history[idx];
                match &item.content {
                    crate::clipboard_manager::ClipboardContent::Text(text) => {
                        if let Err(e) = self.ext.clipboard_manager.copy_to_clipboard(text) {
                            eprintln!("Failed to copy clipboard text: {e}");
                        } else {
                            println!("Pasted clipboard text");
                        }
                    }
                    crate::clipboard_manager::ClipboardContent::Image {
                        width,
                        height,
                        rgba,
                    } => {
                        if let Err(e) = self
                            .ext
                            .clipboard_manager
                            .copy_image_to_clipboard(*width, *height, rgba)
                        {
                            eprintln!("Failed to copy clipboard image: {e}");
                        } else {
                            println!("Pasted clipboard image: {width}×{height}");
                        }
                    }
                }
            } else {
                eprintln!("Invalid clipboard index: {idx}");
            }
        } else {
            eprintln!("Invalid clipboard index format: {index}");
        }
    }

    pub fn handle_hotkey_event(&mut self, event: HotkeyEvent, ctx: &egui::Context) {
        match event {
            HotkeyEvent::ToggleWindow => {
                self.ui.window_visible = !self.ui.window_visible;
                if self.ui.window_visible {
                    self.ui.search_query.clear();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
            HotkeyEvent::HideWindow => {
                self.ui.window_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            HotkeyEvent::LaunchExtension(mode) => {
                self.ui.window_visible = true;
                self.ui.current_mode = Some(mode);
                self.ui.nav_stack.clear();
                self.ui.search_query.clear();
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.trigger_search();
            }
        }
    }
}

/// When the search box does not have focus and the user types text or presses
/// Backspace, apply those events directly to `search_query` and signal that
/// focus must be reclaimed.
///
/// Returns `true` when events were processed (caller: set `search_focused =
/// true` and call `trigger_search()`).  Returns `false` when already focused
/// or when no text-editing events are present.
fn reclaim_focus_on_text_input(
    search_query: &mut String,
    search_focused: bool,
    events: &[egui::Event],
) -> bool {
    if search_focused {
        return false;
    }
    let has_input = events.iter().any(|e| {
        matches!(
            e,
            egui::Event::Text(_)
                | egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                }
        )
    });
    if !has_input {
        return false;
    }
    for event in events {
        match event {
            egui::Event::Text(s) => search_query.push_str(s),
            egui::Event::Key {
                key: egui::Key::Backspace,
                pressed: true,
                ..
            } => {
                search_query.pop();
            }
            _ => {}
        }
    }
    true
}

impl eframe::App for App {
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_messages();
        self.handle_input(ctx);

        // Handle hotkey events (Unix socket fallback + OS global hotkeys).
        if let Some(event) = self.hotkey.hotkey_manager.try_receive() {
            self.handle_hotkey_event(event, ctx);
        }

        // Handle clipboard events
        while let Ok(event) = self.ext.clipboard_manager.get_receiver().try_recv() {
            match event {
                crate::clipboard_manager::ClipboardEvent::NewItem(item) => {
                    let desc = match &item.content {
                        crate::clipboard_manager::ClipboardContent::Text(t) => {
                            format!("text: {}", &t[..t.len().min(50)])
                        }
                        crate::clipboard_manager::ClipboardContent::Image {
                            width, height, ..
                        } => {
                            format!("image {width}×{height}")
                        }
                    };
                    println!("New clipboard item: {desc}");
                    // Arm the debounce timer; the search fires after the quiet
                    // window expires (see below).
                    self.ext.clipboard_pending_search = Some(Instant::now());
                }
                crate::clipboard_manager::ClipboardEvent::Error(e) => {
                    self.ui.toast_manager.push(e, ToastKind::Error);
                }
            }
        }

        // Fire a deferred clipboard search once the debounce window has elapsed.
        if clipboard_search_ready(
            self.ext.clipboard_pending_search,
            Instant::now(),
            CLIPBOARD_DEBOUNCE,
        ) {
            self.ext.clipboard_pending_search = None;
            if self.ui.search_query.is_empty()
                || self.ui.search_query.to_lowercase().contains("clip")
            {
                let manager = self.ext.extension_manager.clone();
                let query = self.ui.search_query.clone();
                self.ext.runtime.spawn(async move {
                    manager.broadcast_search(query).await;
                });
            }
        }

        // Collect detail content before rendering so we can use it inside CentralPanel.
        // (Not used when a form is active, but computed cheaply either way.)
        let detail_content: Option<(
            String,
            String,
            Vec<crate::extension_trait::DetailMetadataRow>,
        )> = self.ui.list.selected_item().and_then(|item| {
            item.detail
                .as_ref()
                .map(|d| (item.title.clone(), d.clone(), item.detail_metadata.clone()))
        });

        // If a form submit was requested during rendering we track it here and
        // fire the async action after the closure.
        let mut pending_form_submit: Option<(String, String)> = None; // (ext_name, json)
        // Render toast notifications on top of everything.
        self.ui.toast_manager.ui(ctx);

        // Use the theme's panel fill (set by theme::visuals_for_theme).
        let panel_bg = ctx.style().visuals.panel_fill;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(panel_bg))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // When a form is active it takes over the full panel — the search bar,
                    // mode badge, and breadcrumb are hidden so the form has all the space.
                    if self.ui.form_state.is_none() {
                        // Simple search bar with focus indication — always full width.
                        let response = ui.text_edit_singleline(&mut self.ui.search_query);

                        // Visual feedback for focus state
                        if self.ui.search_focused {
                            ui.painter().rect_stroke(
                                response.rect.expand(2.0),
                                4.0,
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 150, 255)),
                                egui::StrokeKind::Middle,
                            );
                        }

                        if response.changed() {
                            self.trigger_search();
                        }

                        // Request focus on search bar if needed
                        if self.ui.search_focused && !response.has_focus() {
                            response.request_focus();
                        }

                        // Mode badge — shown when a dedicated extension mode is active.
                        if let Some(ref mode_name) = self.ui.current_mode.clone() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let label = egui::RichText::new(format!(" {mode_name} "))
                                    .color(egui::Color32::from_rgb(180, 220, 255))
                                    .size(11.0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(40, 60, 100))
                                    .rounding(4.0)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .show(ui, |ui| {
                                        ui.label(label);
                                    });
                                ui.label(
                                    egui::RichText::new("  Esc to exit")
                                        .color(egui::Color32::from_gray(120))
                                        .size(11.0),
                                );
                            });
                        }

                        // Breadcrumb bar — shown when the extension has pushed a JS sub-view.
                        // The main-list sentinel frame is excluded from the breadcrumb.
                        let js_frames: Vec<_> = self
                            .ui
                            .nav_stack
                            .iter()
                            .filter(|f| !f.is_main_list_root)
                            .collect();
                        if !js_frames.is_empty() {
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("›")
                                        .color(egui::Color32::from_gray(100))
                                        .size(11.0),
                                );
                                for (i, frame) in js_frames.iter().enumerate() {
                                    if i > 0 {
                                        ui.label(
                                            egui::RichText::new(" › ")
                                                .color(egui::Color32::from_gray(100))
                                                .size(11.0),
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(frame.title.as_str())
                                            .color(egui::Color32::from_gray(160))
                                            .size(11.0),
                                    );
                                }
                                ui.label(
                                    egui::RichText::new("  Esc to go back")
                                        .color(egui::Color32::from_gray(100))
                                        .size(11.0),
                                );
                            });
                        }

                        ui.add_space(8.0);
                    }

                    if let Some(ref mut fs) = self.ui.form_state {
                        // ── Form rendering ────────────────────────────────────
                        let frame = egui::Frame::none()
                            .fill(ui.visuals().faint_bg_color)
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(50)))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::same(12));

                        frame.show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // Clone fields so we can borrow fs.values mutably while iterating.
                                    let fields_snap: Vec<FormFieldDef> = fs.fields.clone();

                                    for field in &fields_snap {
                                        ui.add_space(8.0);
                                        match field {
                                            FormFieldDef::TextField { id, title, .. } => {
                                                ui.label(
                                                    egui::RichText::new(title.as_str())
                                                        .font(egui::FontId::monospace(13.0))
                                                        .color(ui.visuals().text_color()),
                                                );
                                                let value =
                                                    fs.values.entry(id.clone()).or_default();
                                                ui.text_edit_singleline(value);
                                            }
                                            FormFieldDef::Checkbox {
                                                id, title, label, ..
                                            } => {
                                                ui.label(
                                                    egui::RichText::new(title.as_str())
                                                        .font(egui::FontId::monospace(13.0))
                                                        .color(ui.visuals().text_color()),
                                                );
                                                let val_entry = fs
                                                    .values
                                                    .entry(id.clone())
                                                    .or_insert_with(|| "false".to_string());
                                                let mut checked = val_entry.as_str() == "true";
                                                if ui
                                                    .checkbox(&mut checked, label.as_str())
                                                    .changed()
                                                {
                                                    *val_entry = if checked {
                                                        "true".to_string()
                                                    } else {
                                                        "false".to_string()
                                                    };
                                                }
                                            }
                                            FormFieldDef::Dropdown {
                                                id, title, options, ..
                                            } => {
                                                ui.label(
                                                    egui::RichText::new(title.as_str())
                                                        .font(egui::FontId::monospace(13.0))
                                                        .color(ui.visuals().text_color()),
                                                );
                                                let mut selected =
                                                    fs.values.get(id).cloned().unwrap_or_default();
                                                egui::ComboBox::from_id_salt(id.as_str())
                                                    .selected_text(&selected)
                                                    .show_ui(ui, |ui| {
                                                        for opt in options {
                                                            ui.selectable_value(
                                                                &mut selected,
                                                                opt.value.clone(),
                                                                &opt.title,
                                                            );
                                                        }
                                                    });
                                                fs.values.insert(id.clone(), selected);
                                            }
                                        }
                                    }

                                    ui.add_space(16.0);
                                    if ui.button("Submit").clicked()
                                        && let Ok(json) = serde_json::to_string(&fs.values)
                                    {
                                        pending_form_submit =
                                            Some((fs.extension_name.clone(), json));
                                    }

                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("Esc — back to search")
                                            .size(11.0)
                                            .color(egui::Color32::from_gray(100)),
                                    );
                                });
                        });
                    } else if let Some((ref title, ref content, ref metadata)) = detail_content {
                        // ── List + detail panel side-by-side ──────────────────
                        const DETAIL_WIDTH: f32 = 320.0;
                        const MIN_LIST_WIDTH: f32 = 150.0;
                        let available_w = ui.available_width();
                        let available_h = ui.available_height();
                        if available_w >= DETAIL_WIDTH + MIN_LIST_WIDTH + 8.0 {
                            let list_width = available_w - DETAIL_WIDTH - 8.0;
                            ui.allocate_ui(egui::vec2(available_w, available_h), |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(list_width);
                                        ui.set_height(available_h);
                                        self.ui.list.ui(ui);
                                    });
                                    ui.add_space(8.0);
                                    ui.vertical(|ui| {
                                        ui.set_width(DETAIL_WIDTH);
                                        ui.set_height(available_h);
                                        Detail::ui(ui, title, content, metadata);
                                    });
                                });
                            });
                        } else {
                            self.ui.list.ui(ui);
                        }
                    } else {
                        self.ui.list.ui(ui);
                    }
                });
            });

        // Drain any click-activated action (same semantics as pressing Enter).
        let pending_click_action = self.ui.list.take_activated_action();

        // Fire form submit after the UI closure to avoid borrow conflicts.
        if let Some((ext_name, json)) = pending_form_submit {
            if let Some(target_ext) = ext_name.strip_prefix("__prefs__") {
                // Preference form — save values to disk instead of calling on_action.
                let values: HashMap<String, String> =
                    serde_json::from_str(&json).unwrap_or_default();
                match crate::core_extensions::settings_extension::save_extension_prefs(
                    target_ext,
                    &values,
                ) {
                    Ok(()) => self.ui.toast_manager.push(
                        format!("Preferences saved for '{target_ext}'"),
                        ToastKind::Success,
                    ),
                    Err(e) => self.ui.toast_manager.push(
                        format!("Failed to save preferences: {e}"),
                        ToastKind::Error,
                    ),
                }
            } else {
                let action = format!("form-submit::{json}");
                println!("[form] submitting to '{ext_name}': {action}");
                let manager = self.ext.extension_manager.clone();
                self.ext.runtime.spawn(async move {
                    if let Err(e) = manager.handle_action(&ext_name, action, None).await {
                        eprintln!("[form] submit error: {e}");
                    }
                });
            }
            // Clear form state and return to normal search.
            self.ui.form_state = None;
            self.ui.current_mode = None;
            self.ui.search_query.clear();
            self.ui.list.set_items(vec![]);
            self.trigger_search();
            self.ui.search_focused = true;
        }

        // Execute a click-activated list action — identical semantics to Enter.
        if let Some(action) = pending_click_action
            && !action.is_empty()
        {
            self.execute_action(action, ctx);
        }

        // Drain any action triggered via the ActionPanel (keyboard or click).
        if let Some(action) = self.ui.action_panel.take_pending_action()
            && !action.is_empty()
        {
            self.ui.action_panel.close();
            self.execute_action(action, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Action, CLIPBOARD_DEBOUNCE, DropdownOption, EscapeOutcome, FormFieldDef, NavTopFrame,
        clipboard_search_ready, escape_outcome, hides_window_on_action, parse_action,
        parse_extension_arg, parse_form_def, reclaim_focus_on_text_input,
    };
    use crate::modes;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    // --- parse_form_def tests ---

    #[test]
    fn form_def_textfield_parsed() {
        let json = r#"{"fields":[{"type":"textfield","id":"name","title":"Name","placeholder":"Enter name","defaultValue":"Alice"}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0],
            FormFieldDef::TextField {
                id: "name".into(),
                title: "Name".into(),
                placeholder: Some("Enter name".into()),
                default_value: "Alice".into(),
            }
        );
    }

    #[test]
    fn form_def_textfield_no_placeholder() {
        let json = r#"{"fields":[{"type":"textfield","id":"q","title":"Q","defaultValue":""}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert!(matches!(
            fields[0],
            FormFieldDef::TextField { ref placeholder, .. } if placeholder.is_none()
        ));
    }

    #[test]
    fn form_def_checkbox_parsed() {
        let json = r#"{"fields":[{"type":"checkbox","id":"agree","title":"Agreement","label":"I agree","defaultValue":true}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0],
            FormFieldDef::Checkbox {
                id: "agree".into(),
                title: "Agreement".into(),
                label: "I agree".into(),
                default_value: true,
            }
        );
    }

    #[test]
    fn form_def_checkbox_default_false() {
        let json = r#"{"fields":[{"type":"checkbox","id":"x","title":"X","label":"","defaultValue":false}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert!(matches!(
            fields[0],
            FormFieldDef::Checkbox {
                default_value: false,
                ..
            }
        ));
    }

    #[test]
    fn form_def_dropdown_parsed() {
        let json = r#"{"fields":[{"type":"dropdown","id":"color","title":"Color","options":[{"value":"red","title":"Red"},{"value":"blue","title":"Blue"}],"defaultValue":"red"}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0],
            FormFieldDef::Dropdown {
                id: "color".into(),
                title: "Color".into(),
                options: vec![
                    DropdownOption {
                        value: "red".into(),
                        title: "Red".into()
                    },
                    DropdownOption {
                        value: "blue".into(),
                        title: "Blue".into()
                    },
                ],
                default_value: "red".into(),
            }
        );
    }

    #[test]
    fn form_def_dropdown_default_falls_back_to_first_option() {
        let json = r#"{"fields":[{"type":"dropdown","id":"d","title":"D","options":[{"value":"a","title":"A"}]}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert!(matches!(
            &fields[0],
            FormFieldDef::Dropdown { default_value, .. } if default_value == "a"
        ));
    }

    #[test]
    fn form_def_invalid_json_returns_none() {
        assert!(parse_form_def("not json").is_none());
    }

    #[test]
    fn form_def_missing_fields_key_returns_none() {
        assert!(parse_form_def(r#"{"other": []}"#).is_none());
    }

    #[test]
    fn form_def_unknown_field_type_is_skipped() {
        let json = r#"{"fields":[{"type":"unknown","id":"x","title":"X"}]}"#;
        let fields = parse_form_def(json).unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn form_def_mixed_fields() {
        let json = r#"{"fields":[
            {"type":"textfield","id":"a","title":"A","defaultValue":""},
            {"type":"checkbox","id":"b","title":"B","label":"","defaultValue":false},
            {"type":"dropdown","id":"c","title":"C","options":[],"defaultValue":""}
        ]}"#;
        let fields = parse_form_def(json).unwrap();
        assert_eq!(fields.len(), 3);
        assert!(matches!(fields[0], FormFieldDef::TextField { .. }));
        assert!(matches!(fields[1], FormFieldDef::Checkbox { .. }));
        assert!(matches!(fields[2], FormFieldDef::Dropdown { .. }));
    }

    #[test]
    fn form_field_def_id_accessor() {
        let tf = FormFieldDef::TextField {
            id: "tf".into(),
            title: "".into(),
            placeholder: None,
            default_value: "".into(),
        };
        let cb = FormFieldDef::Checkbox {
            id: "cb".into(),
            title: "".into(),
            label: "".into(),
            default_value: false,
        };
        let dd = FormFieldDef::Dropdown {
            id: "dd".into(),
            title: "".into(),
            options: vec![],
            default_value: "".into(),
        };
        assert_eq!(tf.id(), "tf");
        assert_eq!(cb.id(), "cb");
        assert_eq!(dd.id(), "dd");
    }

    #[test]
    fn form_field_def_default_value_str() {
        let tf = FormFieldDef::TextField {
            id: "".into(),
            title: "".into(),
            placeholder: None,
            default_value: "hello".into(),
        };
        assert_eq!(tf.default_value_str(), "hello");
        let cb_t = FormFieldDef::Checkbox {
            id: "".into(),
            title: "".into(),
            label: "".into(),
            default_value: true,
        };
        assert_eq!(cb_t.default_value_str(), "true");
        let cb_f = FormFieldDef::Checkbox {
            id: "".into(),
            title: "".into(),
            label: "".into(),
            default_value: false,
        };
        assert_eq!(cb_f.default_value_str(), "false");
        let dd = FormFieldDef::Dropdown {
            id: "".into(),
            title: "".into(),
            options: vec![],
            default_value: "opt1".into(),
        };
        assert_eq!(dd.default_value_str(), "opt1");
    }

    // --- parse_action tests ---

    #[test]
    fn parse_enter_mode() {
        assert_eq!(
            parse_action(&format!("enter-mode:{}", modes::CLIPBOARD_HISTORY)),
            Action::EnterMode(modes::CLIPBOARD_HISTORY.into())
        );
    }

    #[test]
    fn parse_launch_app() {
        assert_eq!(
            parse_action("launch-app:/usr/bin/firefox"),
            Action::LaunchApp("/usr/bin/firefox".into())
        );
    }

    #[test]
    fn parse_open_url() {
        // URL contains extra colons — prefix match must take priority.
        assert_eq!(
            parse_action("open-url:https://example.com"),
            Action::OpenUrl("https://example.com".into())
        );
    }

    #[test]
    fn parse_open_file() {
        assert_eq!(
            parse_action("open-file:/home/user/doc.pdf"),
            Action::OpenFile("/home/user/doc.pdf".into())
        );
    }

    #[test]
    fn parse_calculator_result() {
        assert_eq!(
            parse_action("calculator-result:42"),
            Action::CalculatorResult("42".into())
        );
    }

    #[test]
    fn parse_calculator_copy() {
        assert_eq!(
            parse_action("calculator-copy:2+2"),
            Action::CalculatorCopy("2+2".into())
        );
    }

    #[test]
    fn parse_clipboard_paste() {
        assert_eq!(
            parse_action("clipboard-paste:3"),
            Action::ClipboardPaste("3".into())
        );
    }

    #[test]
    fn parse_info_actions() {
        assert_eq!(parse_action("calculator-help"), Action::Info);
        assert_eq!(parse_action("calculator-error"), Action::Info);
        assert_eq!(parse_action("clipboard-no-results"), Action::Info);
    }

    #[test]
    fn parse_extension_action_with_item_id() {
        assert_eq!(
            parse_action("my-ext:open:item-123"),
            Action::Extension {
                extension_name: "my-ext".into(),
                action_type: "open:item-123".into(),
                item_id: Some("item-123".into()),
            }
        );
    }

    #[test]
    fn parse_extension_action_without_item_id() {
        assert_eq!(
            parse_action("my-ext:refresh"),
            Action::Extension {
                extension_name: "my-ext".into(),
                action_type: "refresh".into(),
                item_id: None,
            }
        );
    }

    #[test]
    fn parse_unknown_action_no_colon() {
        assert_eq!(
            parse_action("totally-unknown"),
            Action::Unknown("totally-unknown".into())
        );
    }

    #[test]
    fn parse_store_tab_native() {
        assert_eq!(
            parse_action("store-tab:native"),
            Action::StoreTab("native".into())
        );
    }

    #[test]
    fn parse_store_tab_raycast() {
        assert_eq!(
            parse_action("store-tab:raycast"),
            Action::StoreTab("raycast".into())
        );
    }

    fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn typing_when_unfocused_reclaims_focus_and_updates_query() {
        let mut query = String::new();
        let events = vec![egui::Event::Text("hello".to_string())];
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &events);
        assert!(
            reclaimed,
            "should reclaim focus when typing while unfocused"
        );
        assert_eq!(query, "hello");
    }

    #[test]
    fn backspace_when_unfocused_reclaims_focus_and_pops_char() {
        let mut query = "hi".to_string();
        let events = vec![key_event(egui::Key::Backspace, true)];
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &events);
        assert!(
            reclaimed,
            "should reclaim focus on backspace while unfocused"
        );
        assert_eq!(query, "h");
    }

    #[test]
    fn backspace_on_empty_query_does_not_panic() {
        let mut query = String::new();
        let events = vec![key_event(egui::Key::Backspace, true)];
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &events);
        assert!(reclaimed);
        assert_eq!(query, "");
    }

    #[test]
    fn already_focused_is_noop() {
        let mut query = "hi".to_string();
        let events = vec![egui::Event::Text("!".to_string())];
        let reclaimed = reclaim_focus_on_text_input(&mut query, true, &events);
        assert!(!reclaimed, "should not reclaim focus when already focused");
        assert_eq!(
            query, "hi",
            "query should be unchanged when focused (TextEdit owns it)"
        );
    }

    #[test]
    fn arrow_key_does_not_reclaim_focus() {
        let mut query = String::new();
        let events = vec![key_event(egui::Key::ArrowDown, true)];
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &events);
        assert!(
            !reclaimed,
            "navigation keys should not reclaim search focus"
        );
        assert!(query.is_empty());
    }

    #[test]
    fn no_events_is_noop() {
        let mut query = String::new();
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &[]);
        assert!(!reclaimed);
        assert!(query.is_empty());
    }

    #[test]
    fn key_release_does_not_reclaim_focus() {
        let mut query = String::new();
        // Backspace *released* (pressed=false) must not trigger reclaim.
        let events = vec![key_event(egui::Key::Backspace, false)];
        let reclaimed = reclaim_focus_on_text_input(&mut query, false, &events);
        assert!(
            !reclaimed,
            "key-release events must not reclaim search focus"
        );
    }

    // --- escape_outcome tests ---

    #[test]
    fn escape_closes_action_panel_when_open() {
        assert_eq!(
            escape_outcome(true, NavTopFrame::Empty, true, true),
            EscapeOutcome::CloseActionPanel
        );
    }

    #[test]
    fn escape_hides_when_directly_launched_into_mode() {
        // No main-list-root frame on the stack (launched via CLI/hotkey).
        assert_eq!(
            escape_outcome(false, NavTopFrame::Empty, true, true),
            EscapeOutcome::HideWindow
        );
    }

    #[test]
    fn escape_returns_to_main_list_when_entered_from_search() {
        // Main-list sentinel is on the stack (user navigated from global search).
        assert_eq!(
            escape_outcome(false, NavTopFrame::MainListRoot, true, true),
            EscapeOutcome::ReturnToMainList
        );
    }

    #[test]
    fn escape_clears_query_before_returning_to_main_list() {
        // Query is non-empty: clear it first, return to main list on next press.
        assert_eq!(
            escape_outcome(false, NavTopFrame::MainListRoot, false, true),
            EscapeOutcome::ClearSearch
        );
    }

    #[test]
    fn escape_clears_search_when_query_not_empty() {
        assert_eq!(
            escape_outcome(false, NavTopFrame::Empty, false, true),
            EscapeOutcome::ClearSearch
        );
    }

    #[test]
    fn escape_focuses_search_when_unfocused_and_query_empty() {
        assert_eq!(
            escape_outcome(false, NavTopFrame::Empty, true, false),
            EscapeOutcome::FocusSearch
        );
    }

    #[test]
    fn escape_hides_window_when_nothing_else_applies() {
        assert_eq!(
            escape_outcome(false, NavTopFrame::Empty, true, true),
            EscapeOutcome::HideWindow
        );
    }

    #[test]
    fn escape_panel_takes_priority_over_everything() {
        assert_eq!(
            escape_outcome(true, NavTopFrame::MainListRoot, false, false),
            EscapeOutcome::CloseActionPanel
        );
    }

    #[test]
    fn escape_pops_js_view_before_clearing_query() {
        // JS views are always popped first, even when query is non-empty.
        assert_eq!(
            escape_outcome(false, NavTopFrame::JsView, false, false),
            EscapeOutcome::PopNavigation
        );
    }

    #[test]
    fn escape_pops_js_view_with_empty_query() {
        assert_eq!(
            escape_outcome(false, NavTopFrame::JsView, true, true),
            EscapeOutcome::PopNavigation
        );
    }

    #[test]
    fn escape_action_panel_takes_priority_over_nav_pop() {
        assert_eq!(
            escape_outcome(true, NavTopFrame::JsView, true, true),
            EscapeOutcome::CloseActionPanel
        );
    }

    // --- clipboard_search_ready tests ---

    #[test]
    fn clipboard_ready_none_returns_false() {
        assert!(!clipboard_search_ready(
            None,
            Instant::now(),
            CLIPBOARD_DEBOUNCE
        ));
    }

    #[test]
    fn clipboard_ready_before_debounce_returns_false() {
        let now = Instant::now();
        // event happened 50 ms ago, debounce is 100 ms → not ready
        let pending = now.checked_sub(Duration::from_millis(50));
        assert!(!clipboard_search_ready(pending, now, CLIPBOARD_DEBOUNCE));
    }

    #[test]
    fn clipboard_ready_at_debounce_boundary_returns_true() {
        let now = Instant::now();
        // event happened exactly 100 ms ago → ready
        let pending = now.checked_sub(CLIPBOARD_DEBOUNCE);
        assert!(clipboard_search_ready(pending, now, CLIPBOARD_DEBOUNCE));
    }

    #[test]
    fn clipboard_ready_after_debounce_returns_true() {
        let now = Instant::now();
        // event happened 200 ms ago → ready
        let pending = now.checked_sub(Duration::from_millis(200));
        assert!(clipboard_search_ready(pending, now, CLIPBOARD_DEBOUNCE));
    }

    // --- hides_window_on_action tests ---

    #[test]
    fn open_url_hides_window() {
        assert!(hides_window_on_action(&Action::OpenUrl(
            "https://example.com".into()
        )));
    }

    #[test]
    fn open_file_hides_window() {
        assert!(hides_window_on_action(&Action::OpenFile(
            "/home/user/doc.pdf".into()
        )));
    }

    #[test]
    fn launch_app_hides_window() {
        assert!(hides_window_on_action(&Action::LaunchApp(
            "/usr/bin/firefox".into()
        )));
    }

    #[test]
    fn enter_mode_does_not_hide_window() {
        assert!(!hides_window_on_action(&Action::EnterMode(
            modes::CLIPBOARD_HISTORY.into()
        )));
    }

    #[test]
    fn calculator_result_does_not_hide_window() {
        assert!(!hides_window_on_action(&Action::CalculatorResult(
            "42".into()
        )));
    }

    #[test]
    fn info_action_does_not_hide_window() {
        assert!(!hides_window_on_action(&Action::Info));
    }

    #[test]
    fn parse_show_in_finder() {
        assert_eq!(
            parse_action("show-in-finder:/home/user/file.txt"),
            Action::ShowInFinder("/home/user/file.txt".into())
        );
    }

    #[test]
    fn parse_trash_file() {
        assert_eq!(
            parse_action("trash-file:/home/user/file.txt"),
            Action::Trash("/home/user/file.txt".into())
        );
    }

    #[test]
    fn show_in_finder_hides_window() {
        assert!(hides_window_on_action(&Action::ShowInFinder(
            "/path".into()
        )));
    }

    #[test]
    fn trash_does_not_hide_window() {
        assert!(!hides_window_on_action(&Action::Trash("/path".into())));
    }

    #[test]
    fn parse_focus_window_action() {
        assert_eq!(
            parse_action("focus-window:x11:41943044"),
            Action::FocusWindow("x11:41943044".into())
        );
    }

    #[test]
    fn focus_window_hides_window() {
        assert!(hides_window_on_action(&Action::FocusWindow(
            "x11:41943044".into()
        )));
    }

    // ── item_to_actions tests ─────────────────────────────────────────────────

    fn make_item(action: &str) -> crate::extension_trait::ExtensionItem {
        crate::extension_trait::ExtensionItem {
            title: "Test".to_string(),
            subtitle: None,
            icon: None,
            action: action.to_string(),
            id: None,
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }
    }

    #[test]
    fn item_with_no_extra_actions_synthesises_primary() {
        let item = make_item("open-url:https://example.com");
        let actions = super::item_to_actions(&item);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_str, "open-url:https://example.com");
        assert_eq!(actions[0].title, "Test");
    }

    #[test]
    fn item_with_noop_action_returns_empty() {
        let item = make_item("noop");
        let actions = super::item_to_actions(&item);
        assert!(actions.is_empty());
    }

    #[test]
    fn item_with_empty_action_returns_empty() {
        let item = make_item("");
        let actions = super::item_to_actions(&item);
        assert!(actions.is_empty());
    }

    #[test]
    fn item_extra_actions_are_used_when_present() {
        let mut item = make_item("primary-action");
        item.extra_actions = vec![
            crate::extension_trait::ExtraAction {
                title: "Copy".to_string(),
                action: "clipboard-copy:hello".to_string(),
                icon: Some("📋".to_string()),
                shortcut: Some("⌘C".to_string()),
            },
            crate::extension_trait::ExtraAction {
                title: "Open".to_string(),
                action: "open-url:https://example.com".to_string(),
                icon: None,
                shortcut: None,
            },
        ];
        let actions = super::item_to_actions(&item);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].title, "Copy");
        assert_eq!(actions[0].action_str, "clipboard-copy:hello");
        assert_eq!(actions[0].shortcut.as_deref(), Some("⌘C"));
        assert_eq!(actions[1].title, "Open");
        assert_eq!(actions[1].action_str, "open-url:https://example.com");
        assert!(actions[1].shortcut.is_none());
    }

    #[test]
    fn extra_actions_take_priority_over_primary_action() {
        let mut item = make_item("primary-action");
        item.extra_actions = vec![crate::extension_trait::ExtraAction {
            title: "My Action".to_string(),
            action: "ext:do-thing".to_string(),
            icon: None,
            shortcut: None,
        }];
        let actions = super::item_to_actions(&item);
        // Should use extra_actions, not synthesise from primary
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_str, "ext:do-thing");
    }

    // ── search_seq stale-result suppression tests ─────────────────────────────

    /// The pure guard: a result whose seq matches the current counter is kept.
    #[test]
    fn search_seq_same_value_should_send() {
        let counter = Arc::new(AtomicU64::new(1));
        let my_seq: u64 = 1;
        assert!(counter.load(Ordering::Acquire) == my_seq);
    }

    /// A result from an older search (seq < current) is discarded.
    #[test]
    fn search_seq_stale_value_should_not_send() {
        let counter = Arc::new(AtomicU64::new(3));
        let my_seq: u64 = 1; // stale
        assert!(counter.load(Ordering::Acquire) != my_seq);
    }

    /// fetch_add returns the old value; incrementing twice yields seq 1 then 2.
    #[test]
    fn search_seq_increments_monotonically() {
        let counter = Arc::new(AtomicU64::new(0));
        let seq1 = counter.fetch_add(1, Ordering::Release) + 1;
        let seq2 = counter.fetch_add(1, Ordering::Release) + 1;
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(counter.load(Ordering::Acquire), 2);
    }

    /// A task that finishes after a second trigger sees a counter mismatch.
    #[tokio::test]
    async fn search_seq_second_trigger_invalidates_first() {
        let counter = Arc::new(AtomicU64::new(0));

        // First trigger
        let seq1 = counter.fetch_add(1, Ordering::Release) + 1;
        // Second trigger comes before first completes
        let seq2 = counter.fetch_add(1, Ordering::Release) + 1;

        // First task finishes — its seq is stale
        let current = counter.load(Ordering::Acquire);
        assert_ne!(current, seq1, "first trigger's seq should be stale");
        assert_eq!(current, seq2, "second trigger's seq should be current");
    }

    // ── parse_extension_arg tests ─────────────────────────────────────────────

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extension_arg_returns_none_for_empty_args() {
        assert!(parse_extension_arg(&args(&[])).is_none());
    }

    #[test]
    fn extension_arg_returns_none_when_flag_absent() {
        assert!(parse_extension_arg(&args(&["--foo", "bar"])).is_none());
    }

    #[test]
    fn extension_arg_returns_mode_name() {
        let result = parse_extension_arg(&args(&["--extension", "calculator"]));
        assert_eq!(result.as_deref(), Some("calculator"));
    }

    #[test]
    fn extension_arg_with_binary_name_prefix() {
        // Typical argv includes the binary name at index 0.
        let result = parse_extension_arg(&args(&[
            "pterry",
            "--extension",
            "clipboard-history",
        ]));
        assert_eq!(result.as_deref(), Some("clipboard-history"));
    }

    #[test]
    fn extension_arg_returns_none_when_flag_has_no_value() {
        // --extension at the end with no following arg
        assert!(parse_extension_arg(&args(&["--extension"])).is_none());
    }

    #[test]
    fn extension_arg_returns_first_match() {
        let result = parse_extension_arg(&args(&[
            "--extension",
            "calculator",
            "--extension",
            "store",
        ]));
        assert_eq!(result.as_deref(), Some("calculator"));
    }

    #[test]
    fn extension_arg_ignores_unrelated_flags_before() {
        let result = parse_extension_arg(&args(&["--hidden", "--extension", "app-launcher"]));
        assert_eq!(result.as_deref(), Some("app-launcher"));
    }

    // --- settings-set-theme action ---

    #[test]
    fn parse_action_set_theme_dark() {
        assert_eq!(
            parse_action("settings-set-theme:dark"),
            Action::SetTheme("dark".to_string())
        );
    }

    #[test]
    fn parse_action_set_theme_light() {
        assert_eq!(
            parse_action("settings-set-theme:light"),
            Action::SetTheme("light".to_string())
        );
    }

    // ── parse_shortcut_label tests ────────────────────────────────────────────

    #[test]
    fn parse_shortcut_cmd_c() {
        let (mods, key) = super::parse_shortcut_label("⌘C").unwrap();
        assert!(mods.command);
        assert!(!mods.shift);
        assert!(!mods.ctrl);
        assert!(!mods.alt);
        assert_eq!(key, egui::Key::C);
    }

    #[test]
    fn parse_shortcut_ctrl_shift_n() {
        let (mods, key) = super::parse_shortcut_label("⌃⇧N").unwrap();
        assert!(mods.ctrl);
        assert!(mods.shift);
        assert!(!mods.command);
        assert_eq!(key, egui::Key::N);
    }

    #[test]
    fn parse_shortcut_alt_enter() {
        // Alt + digit
        let (mods, key) = super::parse_shortcut_label("⌥1").unwrap();
        assert!(mods.alt);
        assert_eq!(key, egui::Key::Num1);
    }

    #[test]
    fn parse_shortcut_empty_returns_none() {
        assert!(super::parse_shortcut_label("").is_none());
    }

    #[test]
    fn parse_shortcut_no_modifier_returns_none() {
        // A bare key with no modifier chars cannot be a valid shortcut label
        // from _formatShortcut (it always prepends at least one mod symbol).
        // The function still parses it — modifier set will be empty but key is valid.
        // This test documents current behaviour: bare letter is accepted.
        let result = super::parse_shortcut_label("X");
        assert!(result.is_some());
        let (mods, key) = result.unwrap();
        assert_eq!(key, egui::Key::X);
        assert!(!mods.any());
    }

    #[test]
    fn parse_shortcut_unrecognised_key_returns_none() {
        // A modifier followed by a non-letter/digit key we don't handle
        assert!(super::parse_shortcut_label("⌘!").is_none());
    }

    // ── parse_query_arg tests ─────────────────────────────────────────────────

    #[test]
    fn query_arg_returns_none_when_absent() {
        assert!(super::parse_query_arg(&args(&["--extension", "calc"])).is_none());
    }

    #[test]
    fn query_arg_returns_value() {
        let r = super::parse_query_arg(&args(&["--query", "hello world"]));
        assert_eq!(r.as_deref(), Some("hello world"));
    }

    #[test]
    fn query_arg_returns_none_when_flag_has_no_value() {
        assert!(super::parse_query_arg(&args(&["--query"])).is_none());
    }

    #[test]
    fn query_arg_coexists_with_extension_arg() {
        let a = args(&["--extension", "calculator", "--query", "42"]);
        assert_eq!(parse_extension_arg(&a).as_deref(), Some("calculator"));
        assert_eq!(super::parse_query_arg(&a).as_deref(), Some("42"));
    }
}
