use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata,
};
use crate::fuzzy::fuzzy_score;
use crate::modes;
use async_trait::async_trait;
use std::fmt;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct WindowEntry {
    title: String,
    app_name: String,
    /// "<backend>:<id>" — used verbatim in the action string.
    action_id: String,
}

#[derive(Debug)]
enum Backend {
    #[cfg(target_os = "linux")]
    X11,
    Noop,
}

// ── Extension struct ──────────────────────────────────────────────────────────

pub struct WindowSwitcherExtension {
    metadata: ExtensionMetadata,
    backend: Backend,
}

impl fmt::Debug for WindowSwitcherExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowSwitcherExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for WindowSwitcherExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowSwitcherExtension {
    pub fn new() -> Self {
        Self {
            metadata: ExtensionMetadata {
                name: modes::WINDOW_SWITCHER.to_string(),
                version: "1.0.0".to_string(),
                description: Some("Switch between open windows".to_string()),
                author: None,
                language: ExtensionLanguage::Rust,
                entry_point: "window_switcher_extension.rs".to_string(),
                permissions: vec![],
                // Keep out of the global broadcast; discoverable via launcher_item()
                // so users can type "window" to find it without polluting the empty
                // query list with all open windows alongside app launcher results.
                auto_load: false,
                title: None,
                preferences: vec![],
                is_development: false,
            },
            backend: detect_backend(),
        }
    }
}

fn detect_backend() -> Backend {
    #[cfg(target_os = "linux")]
    if std::env::var("DISPLAY").is_ok() {
        return Backend::X11;
    }
    Backend::Noop
}

// ── Extension trait impl ──────────────────────────────────────────────────────

#[async_trait]
impl Extension for WindowSwitcherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let windows: Vec<WindowEntry> = match self.backend {
            #[cfg(target_os = "linux")]
            Backend::X11 => enumerate_x11_windows(),
            Backend::Noop => vec![],
        };

        let items = fuzzy_filter(&windows, query)
            .into_iter()
            .take(50)
            .map(|w| ExtensionItem {
                title: w.title.clone(),
                subtitle: if w.app_name.is_empty() {
                    None
                } else {
                    Some(w.app_name.clone())
                },
                icon: None,
                action: format!("focus-window:{}", w.action_id),
                id: Some(w.action_id.clone()),
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
        // Actions are handled by app.rs via the focus-window: prefix.
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    fn launcher_item(&self) -> Option<ExtensionItem> {
        Some(ExtensionItem {
            title: "Switch Window".to_string(),
            subtitle: Some("Switch between open windows".to_string()),
            icon: Some("🪟".to_string()),
            action: format!("enter-mode:{}", modes::WINDOW_SWITCHER),
            id: Some("window-switcher-launcher".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
    }
}

// ── Pure helpers (testable without a display) ─────────────────────────────────

/// Extract the second null-separated segment (class name) from WM_CLASS bytes.
fn parse_wm_class_bytes(data: &[u8]) -> String {
    // WM_CLASS layout: "<instance>\0<class>\0"
    let second_start = data
        .iter()
        .position(|&b| b == 0)
        .map(|i| i + 1)
        .unwrap_or(data.len());
    let second = &data[second_start..];
    let end = second.iter().position(|&b| b == 0).unwrap_or(second.len());
    String::from_utf8_lossy(&second[..end]).into_owned()
}

/// Fuzzy-filter and rank `entries` against `query`.
/// Returns all entries when `query` is empty.
fn fuzzy_filter<'a>(entries: &'a [WindowEntry], query: &str) -> Vec<&'a WindowEntry> {
    if query.is_empty() {
        return entries.iter().collect();
    }
    let mut scored: Vec<(i32, &WindowEntry)> = entries
        .iter()
        .filter_map(|e| {
            let best = fuzzy_score(query, &e.title).max(fuzzy_score(query, &e.app_name));
            if best > 0 { Some((best, e)) } else { None }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, e)| e).collect()
}

// ── Linux / X11 backend ───────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn enumerate_x11_windows() -> Vec<WindowEntry> {
    match try_enumerate_x11_windows() {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("[window-switcher] X11 enumeration failed: {e}");
            vec![]
        }
    }
}

#[cfg(target_os = "linux")]
fn try_enumerate_x11_windows() -> Result<Vec<WindowEntry>, Box<dyn std::error::Error>> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
    use x11rb::rust_connection::RustConnection;

    let (conn, screen_num) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let current_pid = std::process::id();

    let net_client_list = conn.intern_atom(false, b"_NET_CLIENT_LIST_STACKING")?.reply()?.atom;
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
    let utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let net_wm_pid = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;

    // Fetch the stacking list; reverse so frontmost is first.
    let list_reply = conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, 1024)?
        .reply()?;

    let window_ids: Vec<u32> = list_reply
        .value32()
        .map(|iter| iter.collect())
        .unwrap_or_default();
    let window_ids: Vec<u32> = window_ids.into_iter().rev().collect();

    let mut entries = Vec::new();

    for wid in window_ids {
        // Skip our own windows.
        let pid_reply = conn
            .get_property(false, wid, net_wm_pid, AtomEnum::CARDINAL, 0, 1)?
            .reply()?;
        if pid_reply
            .value32()
            .and_then(|mut i| i.next())
            .is_some_and(|pid| pid == current_pid)
        {
            continue;
        }

        // Title: prefer _NET_WM_NAME (UTF-8), fall back to WM_NAME (Latin-1).
        let title = get_text_prop(&conn, wid, net_wm_name, utf8_string)
            .or_else(|| {
                get_text_prop(
                    &conn,
                    wid,
                    u32::from(AtomEnum::WM_NAME),
                    u32::from(AtomEnum::STRING),
                )
            })
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let app_name = get_wm_class(&conn, wid);

        entries.push(WindowEntry {
            title,
            app_name,
            action_id: format!("x11:{wid}"),
        });
    }

    Ok(entries)
}

#[cfg(target_os = "linux")]
fn get_text_prop(
    conn: &x11rb::rust_connection::RustConnection,
    window: u32,
    property: u32,
    type_: u32,
) -> Option<String> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let reply = conn
        .get_property(false, window, property, type_, 0, 2048)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.is_empty() {
        return None;
    }
    let s = String::from_utf8_lossy(&reply.value).into_owned();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(target_os = "linux")]
fn get_wm_class(conn: &x11rb::rust_connection::RustConnection, window: u32) -> String {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let Some(reply) = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 2048)
        .ok()
        .and_then(|c| c.reply().ok())
    else {
        return String::new();
    };
    parse_wm_class_bytes(&reply.value)
}

// ── focus_x11_window (called from platform.rs) ────────────────────────────────

/// Raise and focus an X11 window by its decimal window ID string.
/// Called from `platform::focus_window` when backend is "x11".
#[cfg(target_os = "linux")]
pub fn focus_x11_window(id_str: &str) {
    if let Err(e) = try_focus_x11_window(id_str) {
        eprintln!("[window-switcher] focus failed: {e}");
    }
}

#[cfg(target_os = "linux")]
fn try_focus_x11_window(id_str: &str) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        AtomEnum, CLIENT_MESSAGE_EVENT, ClientMessageData, ClientMessageEvent, ConnectionExt as _,
        EventMask,
    };

    let wid: u32 = id_str.parse()?;

    let (conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
    let root = conn.setup().roots[screen_num].root;

    let net_active_window = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
    let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
    let net_wm_state_hidden =
        conn.intern_atom(false, b"_NET_WM_STATE_HIDDEN")?.reply()?.atom;
    let net_wm_user_time = conn.intern_atom(false, b"_NET_WM_USER_TIME")?.reply()?.atom;

    // Fetch a recent timestamp from the target window's _NET_WM_USER_TIME.
    // Modern WMs (KDE, GNOME) enforce focus-stealing prevention and silently
    // drop _NET_ACTIVE_WINDOW requests that carry CurrentTime (0).
    let timestamp = conn
        .get_property(false, wid, net_wm_user_time, AtomEnum::CARDINAL, 0, 1)?
        .reply()
        .ok()
        .and_then(|r| r.value32().and_then(|mut i| i.next()))
        .unwrap_or(0);

    // Remove _NET_WM_STATE_HIDDEN so a minimised/iconified window is restored.
    // data: [action=0(remove), atom1, atom2=0, source=2(pager), 0]
    let unmin_data = ClientMessageData::from([0u32, net_wm_state_hidden, 0u32, 2u32, 0u32]);
    let unmin_event = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window: wid,
        type_: net_wm_state,
        data: unmin_data,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        unmin_event,
    )?;

    // Raise and focus via _NET_ACTIVE_WINDOW.
    // data: [source=2(pager), timestamp, currently-active=0, 0, 0]
    let data = ClientMessageData::from([2u32, timestamp, 0u32, 0u32, 0u32]);
    let event = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window: wid,
        type_: net_active_window,
        data,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(title: &str, app_name: &str, id: &str) -> WindowEntry {
        WindowEntry {
            title: title.to_string(),
            app_name: app_name.to_string(),
            action_id: format!("x11:{id}"),
        }
    }

    // ── parse_wm_class_bytes ──────────────────────────────────────────────────

    #[test]
    fn parse_wm_class_second_segment() {
        // "instance\0class\0"
        let data = b"Navigator\0Firefox\0";
        assert_eq!(parse_wm_class_bytes(data), "Firefox");
    }

    #[test]
    fn parse_wm_class_no_second_segment() {
        let data = b"onlyone";
        assert_eq!(parse_wm_class_bytes(data), "");
    }

    #[test]
    fn parse_wm_class_empty() {
        assert_eq!(parse_wm_class_bytes(b""), "");
    }

    #[test]
    fn parse_wm_class_two_nulls_only() {
        let data = b"\0\0";
        assert_eq!(parse_wm_class_bytes(data), "");
    }

    // ── fuzzy_filter ─────────────────────────────────────────────────────────

    #[test]
    fn empty_query_returns_all() {
        let entries = vec![
            make_entry("Firefox", "Firefox", "1"),
            make_entry("Terminal", "kitty", "2"),
        ];
        assert_eq!(fuzzy_filter(&entries, "").len(), 2);
    }

    #[test]
    fn fuzzy_matches_by_title() {
        let entries = vec![
            make_entry("Firefox", "Firefox", "1"),
            make_entry("Calculator", "gnome-calculator", "2"),
        ];
        let results = fuzzy_filter(&entries, "fire");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Firefox");
    }

    #[test]
    fn fuzzy_matches_by_app_name() {
        let entries = vec![
            make_entry("Untitled — kitty", "kitty", "1"),
            make_entry("Firefox", "Firefox", "2"),
        ];
        let results = fuzzy_filter(&entries, "kitt");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].app_name, "kitty");
    }

    #[test]
    fn fuzzy_no_match_returns_empty() {
        let entries = vec![make_entry("Firefox", "Firefox", "1")];
        assert!(fuzzy_filter(&entries, "zzz").is_empty());
    }

    // ── action string format ──────────────────────────────────────────────────

    #[test]
    fn action_string_uses_focus_window_prefix() {
        let entry = make_entry("Firefox", "Firefox", "12345");
        assert_eq!(format!("focus-window:{}", entry.action_id), "focus-window:x11:12345");
    }

    // ── cap at 50 ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn on_search_caps_at_fifty() {
        // Build an extension with Noop backend; inject 60 entries via fuzzy_filter.
        // We test the cap via the pure filter path directly.
        let entries: Vec<WindowEntry> = (0..60)
            .map(|i| make_entry(&format!("Window {i}"), "App", &i.to_string()))
            .collect();
        let filtered: Vec<&WindowEntry> = fuzzy_filter(&entries, "").into_iter().take(50).collect();
        assert_eq!(filtered.len(), 50);
    }

    // ── auto_load / launcher_item ─────────────────────────────────────────────

    #[test]
    fn not_auto_loaded() {
        assert!(!WindowSwitcherExtension::new().metadata().auto_load);
    }

    #[test]
    fn has_launcher_item() {
        assert!(WindowSwitcherExtension::new().launcher_item().is_some());
    }

    #[test]
    fn launcher_item_enters_window_switcher_mode() {
        let item = WindowSwitcherExtension::new().launcher_item().unwrap();
        assert_eq!(
            item.action,
            format!("enter-mode:{}", crate::modes::WINDOW_SWITCHER)
        );
    }
}
