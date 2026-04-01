use crossbeam_channel::{Receiver, Sender, bounded};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyEvent {
    ToggleWindow,
    HideWindow,
    /// Open the window directly into the named extension mode.
    LaunchExtension(String),
}

pub struct HotkeyManager {
    sender: Sender<HotkeyEvent>,
    receiver: Receiver<HotkeyEvent>,
    /// Maps OS-level hotkey ID → the event to fire when that hotkey is pressed.
    hotkey_map: HashMap<u32, HotkeyEvent>,
    is_running: Arc<Mutex<bool>>,
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyManager {
    /// Create a manager with no registered global hotkeys (used for testing
    /// and on Wayland where global hotkey registration is unavailable).
    pub fn new() -> Self {
        let (sender, receiver) = bounded(10);
        Self {
            sender,
            receiver,
            hotkey_map: HashMap::new(),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// Create a manager that maps the given OS hotkey IDs to events.
    /// The caller must keep the `GlobalHotKeyManager` alive for the
    /// registrations to remain active.
    pub fn with_hotkey_map(hotkey_map: HashMap<u32, HotkeyEvent>) -> Self {
        let (sender, receiver) = bounded(10);
        Self {
            sender,
            receiver,
            hotkey_map,
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    pub fn start_listening(&self) {
        *self.is_running.lock().unwrap_or_else(|e| e.into_inner()) = true;

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::net::UnixListener;

            let path = socket_path();
            let _ = std::fs::remove_file(&path);
            let sender = self.sender.clone();
            let is_running = self.is_running.clone();

            println!("Launcher socket: {path}");
            println!("Show launcher: echo show | socat - UNIX-CONNECT:{path}");

            std::thread::spawn(move || {
                let listener = match UnixListener::bind(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Failed to bind socket {path}: {e}");
                        return;
                    }
                };

                for stream in listener.incoming() {
                    if !*is_running.lock().unwrap_or_else(|e| e.into_inner()) {
                        break;
                    }
                    match stream {
                        Ok(_) => {
                            let _ = sender.send(HotkeyEvent::ToggleWindow);
                        }
                        Err(e) => eprintln!("Socket error: {e}"),
                    }
                }
            });
        }
    }

    pub fn stop_listening(&self) {
        *self.is_running.lock().unwrap_or_else(|e| e.into_inner()) = false;

        #[cfg(target_os = "linux")]
        {
            let path = socket_path();
            let _ = std::os::unix::net::UnixStream::connect(&path);
            let _ = std::fs::remove_file(&path);
        }
    }

    /// Poll for the next hotkey event, checking both the Unix-socket channel
    /// and the OS global-hotkey event queue.
    pub fn try_receive(&self) -> Option<HotkeyEvent> {
        // Unix socket / simulated events
        if let Ok(event) = self.receiver.try_recv() {
            return Some(event);
        }
        // OS global hotkey events
        if let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv()
            && event.state == global_hotkey::HotKeyState::Pressed
        {
            return self.hotkey_map.get(&event.id).cloned();
        }
        None
    }

    pub fn simulate_hotkey(&self, event: HotkeyEvent) {
        let _ = self.sender.send(event);
    }
}

/// Parse a hotkey string (e.g. `"alt+space"`, `"ctrl+shift+a"`) into a
/// `global_hotkey::hotkey::HotKey` using the crate's built-in `FromStr` parser.
pub fn parse_hotkey_string(s: &str) -> Result<global_hotkey::hotkey::HotKey, String> {
    s.parse::<global_hotkey::hotkey::HotKey>()
        .map_err(|e| e.to_string())
}

/// Register all configured hotkeys with the OS.  Returns a map of
/// `hotkey_id → HotkeyEvent` and the `GlobalHotKeyManager` (must be kept
/// alive for the process lifetime so registrations remain active).
///
/// `toggle_key` is the hotkey string for the main toggle-window action.
/// `extension_hotkeys` maps extension mode names to hotkey strings.
///
/// On failure (e.g. Wayland without XWayland) the map is empty and the
/// manager is `None`.
pub fn register_hotkeys(
    toggle_key: &str,
    extension_hotkeys: &HashMap<String, String>,
) -> (
    HashMap<u32, HotkeyEvent>,
    Option<global_hotkey::GlobalHotKeyManager>,
) {
    use global_hotkey::GlobalHotKeyManager;

    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Global hotkey unavailable: {e}");
            eprintln!("Fallback: echo show | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/launcher.sock");
            return (HashMap::new(), None);
        }
    };

    let mut map: HashMap<u32, HotkeyEvent> = HashMap::new();

    // Register the main toggle hotkey.
    match parse_hotkey_string(toggle_key) {
        Ok(hk) => {
            let id = hk.id();
            match manager.register(hk) {
                Ok(_) => {
                    println!("Global hotkey registered: {toggle_key}");
                    map.insert(id, HotkeyEvent::ToggleWindow);
                }
                Err(e) => eprintln!("Failed to register toggle hotkey '{toggle_key}': {e}"),
            }
        }
        Err(e) => eprintln!("Failed to parse toggle hotkey '{toggle_key}': {e}"),
    }

    // Register per-extension launch hotkeys.
    for (mode_name, hotkey_str) in extension_hotkeys {
        match parse_hotkey_string(hotkey_str) {
            Ok(hk) => {
                let id = hk.id();
                match manager.register(hk) {
                    Ok(_) => {
                        println!("Extension hotkey registered: {hotkey_str} → {mode_name}");
                        map.insert(id, HotkeyEvent::LaunchExtension(mode_name.clone()));
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to register hotkey '{hotkey_str}' for '{mode_name}': {e}"
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse hotkey '{hotkey_str}' for '{mode_name}': {e}");
            }
        }
    }

    (map, Some(manager))
}

#[cfg(target_os = "linux")]
fn socket_path() -> String {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{dir}/launcher.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_receive_empty_returns_none() {
        let manager = HotkeyManager::new();
        assert!(manager.try_receive().is_none());
    }

    #[test]
    fn simulate_toggle_window_is_received() {
        let manager = HotkeyManager::new();
        manager.simulate_hotkey(HotkeyEvent::ToggleWindow);
        assert!(matches!(
            manager.try_receive(),
            Some(HotkeyEvent::ToggleWindow)
        ));
    }

    #[test]
    fn simulate_hide_window_is_received() {
        let manager = HotkeyManager::new();
        manager.simulate_hotkey(HotkeyEvent::HideWindow);
        assert!(matches!(
            manager.try_receive(),
            Some(HotkeyEvent::HideWindow)
        ));
    }

    #[test]
    fn after_receive_queue_is_empty() {
        let manager = HotkeyManager::new();
        manager.simulate_hotkey(HotkeyEvent::ToggleWindow);
        let _ = manager.try_receive();
        assert!(manager.try_receive().is_none());
    }

    #[test]
    fn simulate_launch_extension_is_received() {
        let manager = HotkeyManager::new();
        manager.simulate_hotkey(HotkeyEvent::LaunchExtension("calculator".to_string()));
        assert!(
            matches!(manager.try_receive(), Some(HotkeyEvent::LaunchExtension(m)) if m == "calculator")
        );
    }

    #[test]
    fn parse_hotkey_string_alt_space_is_ok() {
        assert!(parse_hotkey_string("alt+space").is_ok());
    }

    #[test]
    fn parse_hotkey_string_ctrl_shift_a_is_ok() {
        assert!(parse_hotkey_string("ctrl+shift+a").is_ok());
    }

    #[test]
    fn parse_hotkey_string_invalid_returns_err() {
        assert!(parse_hotkey_string("not_a_real_key???").is_err());
    }

    #[test]
    fn with_hotkey_map_empty_try_receive_returns_none() {
        let manager = HotkeyManager::with_hotkey_map(HashMap::new());
        assert!(manager.try_receive().is_none());
    }

    #[test]
    fn simulate_hotkey_still_works_with_hotkey_map() {
        let manager = HotkeyManager::with_hotkey_map(HashMap::new());
        manager.simulate_hotkey(HotkeyEvent::ToggleWindow);
        assert!(matches!(
            manager.try_receive(),
            Some(HotkeyEvent::ToggleWindow)
        ));
    }
}
