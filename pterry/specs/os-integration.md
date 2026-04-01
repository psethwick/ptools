# OS Integration

## Window Behaviour

- Always-on-top: `ViewportCommand::AlwaysOnTop`
- Focus stealing: `ViewportCommand::Focus` called on hotkey trigger
- **Hide-after-action policy**:
  - `LaunchApp` → hide window, clear search
  - `OpenInBrowser` / `OpenFile` → hide window, clear search
  - `CopyToClipboard`, `CalculatorResult` → stay visible, show toast
  - `EnterMode` → stay visible
  - Extension JS callbacks (`onAction`) → stay visible by default; extension may call `raycast.hideWindow()` explicitly

## Global Hotkey System

Implemented via the `global-hotkey` crate (`src/app.rs` + `src/hotkey_manager.rs`).

**Toggle hotkey** (default: Alt+Space on Linux/Windows, Cmd+Space on macOS): toggles window visibility. User-configurable via settings; stored as a human-readable string (`"Alt+Space"`) and parsed to `global_hotkey::hotkey::{Code, Modifiers}` at startup.

**Per-extension hotkeys**: each extension (or command) may have an optional `hotkey` string in its metadata. At startup, all registered hotkeys are collected and registered with `GlobalHotKeyManager`. Events arrive as `GlobalHotKeyEvent` with the hotkey's ID; the handler maps ID → extension name and fires `HotkeyEvent::LaunchExtension(name)`, which shows the window and activates that extension's mode.

**Hotkey string format**: `"<Modifier>+<Key>"`, e.g. `"Alt+Space"`, `"Ctrl+Shift+C"`. Modifiers: `Alt`, `Ctrl`, `Shift`, `Super`/`Cmd`. Keys: standard key names (`Space`, `Return`, `A`–`Z`, `F1`–`F12`, etc.).

**Re-registration**: when the user changes a hotkey in settings, the old hotkey is unregistered and the new one registered without restart.

**Wayland fallback**: when `GlobalHotKeyManager::new()` fails (Wayland without XWayland), a Unix socket listener is started at `$XDG_RUNTIME_DIR/launcher.sock`. Send any byte to toggle the window. The CLI `--show` flag connects to this socket.

## Platform-Specific Effects

- macOS: `window-vibrancy` applies `NSVisualEffectMaterial::AppearanceBased` frosted-glass
- Windows: `window-vibrancy` applies acrylic blur
- Linux: no vibrancy (compositor-dependent); window uses a solid themed background

## Clipboard

- Read: platform clipboard via `arboard`; errors silently swallowed if clipboard is empty
- Write: `arboard` on all platforms; macOS falls back to `pbcopy` if arboard unavailable
- History: polled every 500 ms; last 100 items stored in memory; debounced 100 ms before triggering a search update
