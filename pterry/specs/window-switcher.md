# Window Switcher Extension

A built-in Rust extension that lists open windows and raises/focuses the selected one. Implemented entirely in Rust — no external tools required.

## Behaviour

- `auto_load: true` — window titles appear in global search alongside app launcher results.
- Each result item shows the **window title** as the primary label and the **application name** as the subtitle.
- Selecting an item raises and focuses that window, then hides the launcher.
- Windows belonging to the launcher process itself are excluded.
- Empty query: returns all windows (capped at 50), ordered most-recently-active first where the backend supports it.
- Non-empty query: fuzzy-match on both window title and application name.

## Action Protocol

Action string: `focus-window:<backend>:<window-id>`

- `focus-window:` prefix → `parse_action` → `Action::FocusWindow { backend, id }`.
- `execute_action` calls `crate::platform::focus_window(backend, &id)`.
- `hides_window_on_action` returns `true` for `Action::FocusWindow`.

The `backend` segment encodes which focusing mechanism to use (e.g. `sway`, `hyprland`, `x11`, `wlr`, `macos`, `win32`), so the correct call is made without re-detecting the compositor at action time.

## Backend Detection & Fallback Chain

Detection runs once at `initialize()` time. Only one backend is active per session; the result is stored in the extension struct.

### Linux

```
1. $SWAYSOCK present                       → Sway IPC backend
2. $HYPRLAND_INSTANCE_SIGNATURE present    → Hyprland IPC backend
3. Wayland socket reachable AND
   wlr-foreign-toplevel in globals         → wlr-foreign-toplevel backend
4. $DISPLAY present                        → x11rb EWMH backend
5. Nothing matches                         → NoOp backend
```

### macOS

```
1. Always                                  → NSWorkspace/Accessibility backend (see below)
```

### Windows

```
1. Always                                  → Win32 EnumWindows backend (see below)
```

---

## Linux Backends

### Backend L1 — Sway IPC (`swayipc-async`)

**Trigger:** `$SWAYSOCK` environment variable is set.

**Crate:** `swayipc-async = "3"` (Tokio-compatible).

**Enumerate:**

Call `Connection::get_tree()` and walk the node tree recursively. Collect all leaf nodes where `node.node_type == NodeType::Con` and `node.name` is `Some`. Skip nodes where `node.pid == Some(current_pid)`.

Relevant fields per node:
- `node.id` — i64, the con_id used for focusing
- `node.name` — `Option<String>` — window title
- `node.app_id` — `Option<String>` — Wayland app ID (native Wayland windows)
- `node.window_properties.class` — X11 class (XWayland windows)
- `node.focused` — bool, whether this is the currently focused window

App name: prefer `app_id`, fall back to `window_properties.class`, fall back to empty string.

**Focus:** `Connection::run_command(format!("[con_id={id}] focus"))`.

**Action format:** `focus-window:sway:<con_id>`

---

### Backend L2 — Hyprland IPC

**Trigger:** `$HYPRLAND_INSTANCE_SIGNATURE` environment variable is set.

**Method:** Shell out to `hyprctl clients -j` (`hyprctl` is always present when Hyprland is running; avoids pulling in the `hyprland` crate).

**Enumerate:** Parse JSON array. Each element:
```json
{
  "address": "0x...",
  "title": "...",
  "class": "...",
  "hidden": false,
  "mapped": true,
  "focusHistoryID": 0
}
```
Filter out entries where `hidden` is true, `mapped` is false, or `pid == current_pid`.

App name: `class` field. Sort by `focusHistoryID` ascending (0 = most recently focused).

**Focus:** `hyprctl dispatch focuswindow address:<address>` via `Command`.

**Action format:** `focus-window:hyprland:<address>` (e.g. `focus-window:hyprland:0x55f3a2b1c0`)

---

### Backend L3 — wlr-foreign-toplevel (`wayland-client`)

**Trigger:** Wayland socket reachable (`$WAYLAND_DISPLAY` or default `wayland-0`) AND compositor advertises `zwlr_foreign_toplevel_manager_v1` in its global registry.

**Crates:**
```toml
wayland-client = "0.31"
wayland-protocols-wlr = "0.3"
```

**Compositor coverage:** Sway, Hyprland, KWin (KDE), Mutter (GNOME), niri, COSMIC, river, Wayfire, Weston, Mir, GameScope.

**Enumerate:**

Spawn a dedicated Tokio task at `initialize()` that:
1. Connects to the Wayland display.
2. Binds `zwlr_foreign_toplevel_manager_v1` and `wl_seat` from the global registry.
3. Dispatches events until `Done` is received for each handle; collects title, app_id, state (skip minimized, skip this process's PID via `_NET_WM_PID` if available).
4. Sends the collected `Vec<WindowEntry>` back via a `tokio::sync::watch` channel.
5. Continues listening for `Closed` events to keep the list live.

`on_search` reads the latest snapshot from the watch channel receiver.

**Focus:** Send a `FocusRequest(handle_id)` over a `tokio::sync::mpsc` channel to the Wayland task, which calls `handle.activate(seat)` and flushes. Handle objects must remain on the Wayland task thread.

**Action format:** `focus-window:wlr:<handle_id>` where `handle_id` is a `u32` assigned sequentially as handles arrive.

---

### Backend L4 — X11 EWMH (`x11rb`)

**Trigger:** `$DISPLAY` is set.

**Crate:**
```toml
x11rb = { version = "0.13", features = ["allow-unsafe-code"] }
```

**Enumerate:**

1. Connect: `RustConnection::connect(None)`.
2. Intern atoms: `_NET_CLIENT_LIST_STACKING`, `_NET_WM_NAME`, `WM_NAME`, `WM_CLASS`, `_NET_WM_PID`, `UTF8_STRING`.
3. Read `_NET_CLIENT_LIST_STACKING` on the root window → list of `Window` (u32) IDs, front-to-back.
4. For each window:
   - `_NET_WM_NAME` (UTF-8) or fall back to `WM_NAME` → title
   - `WM_CLASS` → two null-separated strings; second segment is app name
   - `_NET_WM_PID` → skip if matches current PID

**Focus:** Send `_NET_ACTIVE_WINDOW` `ClientMessage` to root:
```
data[0]: 2   (source: pager/launcher)
data[1]: 0   (timestamp)
data[2]: 0   (current active window)
data[3..]: 0
```
Event mask: `SUBSTRUCTURE_REDIRECT | SUBSTRUCTURE_NOTIFY`. Then flush.

**Action format:** `focus-window:x11:<window_id>` (decimal u32)

---

### Backend L5 — NoOp

When no Linux backend is detected, `on_search` returns a single informational item ("Window switching unavailable on this compositor") with action `window-switcher-unavailable` (maps to `Action::Info`). `initialize()` logs the situation with `eprintln!`.

---

## macOS Backend (`NSWorkspace` / Accessibility API)

> **Status: not yet implemented.** Stubbed to return empty. Gated behind `#[cfg(target_os = "macos")]`.

**Enumerate:** Use `NSWorkspace::sharedWorkspace().runningApplications()` to get all running apps, then use the Accessibility API (`AXUIElement`) to enumerate windows per process. Each window yields a title (`AXTitle`) and the owning app's `localizedName`.

**Focus:** `NSRunningApplication::activateWithOptions(NSApplicationActivateIgnoringOtherApps)`, then `AXUIElement::setAttributeValue(AXFrontmost, true)` on the target window.

**Action format:** `focus-window:macos:<pid>:<window_index>`

**Crate:** `objc2` + `objc2-app-kit`, already gated behind the `macos` feature flag used elsewhere in the project.

---

## Windows Backend (`EnumWindows`)

> **Status: not yet implemented.** Stubbed to return empty. Gated behind `#[cfg(target_os = "windows")]`.

**Enumerate:** Call `EnumWindows` with a callback that collects `HWND`s. For each:
- `IsWindowVisible(hwnd)` — skip invisible
- `GetWindowTextW(hwnd)` → title; skip if empty
- `GetWindowThreadProcessId(hwnd, &pid)` → skip if pid matches current
- `GetWindowModuleFileNameW` or process name via `OpenProcess` + `GetModuleFileNameEx` → app name

**Focus:** `SetForegroundWindow(hwnd)` + `ShowWindow(hwnd, SW_RESTORE)` if minimized.

**Action format:** `focus-window:win32:<hwnd>` (hwnd as decimal u64)

**Crate:** `windows` crate (`windows::Win32::UI::WindowsAndMessaging`), already used elsewhere if/when Windows support lands.

---

## Mode & Registration

Mode name constant: `modes::WINDOW_SWITCHER = "window-switcher"` in `src/modes.rs`.

Registered in `ExtensionManager::load_builtin_extensions()` alongside app launcher, calculator, and clipboard history.

`auto_load: true` — results appear in global search; no dedicated launcher item needed.

---

## Cargo Dependencies

```toml
# Linux only
[target.'cfg(target_os = "linux")'.dependencies]
swayipc-async = { version = "3", optional = true }
x11rb = { version = "0.13", features = ["allow-unsafe-code"], optional = true }
wayland-client = { version = "0.31", optional = true }
wayland-protocols-wlr = { version = "0.3", optional = true }

# macOS — objc2 already present under the `macos` feature; no new deps needed
# Windows — `windows` crate added when Windows support lands
```

A `linux-window-switcher` cargo feature enables all four Linux crates. macOS and Windows backends compile in automatically on their respective platforms once implemented.

---

## Error Handling

| Situation | Handling |
|-----------|----------|
| `hyprctl` not in `$PATH` | Fall through to next backend at init |
| Sway IPC connection refused | Fall through to next backend at init |
| Wayland connect fails | Fall through to x11rb |
| x11rb connect fails | Fall through to NoOp |
| `hyprctl` spawn fails at focus time | `eprintln!`, no UI error |
| `_NET_ACTIVE_WINDOW` send fails | `eprintln!`, no UI error |
| wlr handle already closed at focus time | `eprintln!`, no UI error |
| NSWorkspace / AXUIElement unavailable | Return empty, `eprintln!` |
| `EnumWindows` fails | Return empty, `eprintln!` |
| Malformed `hyprctl` JSON | Skip malformed entries, log with `eprintln!` |
| X11 window property fetch error | Skip that window, continue |

---

## Tests

Tests live inline under `#[cfg(test)]` in `src/core_extensions/window_switcher_extension.rs`.

### Pure-logic unit tests (no compositor or OS needed)

- `parse_hyprctl_json` — valid JSON yields correct `WindowEntry` vec
- `parse_hyprctl_json` — `hidden: true` entries are filtered out
- `parse_hyprctl_json` — `mapped: false` entries are filtered out
- `parse_hyprctl_json` — malformed JSON returns empty (no panic)
- `parse_x11_wm_class` — two null-separated bytes yield `(instance, class)` tuple
- `parse_x11_wm_class` — single string (missing second null) returns empty class
- `fuzzy_filter` — `"fire"` matches entry with title `"Firefox"`
- `fuzzy_filter` — `"fire"` matches entry with app_name `"Firefox"` even if title differs
- `fuzzy_filter` — `"zzz"` returns empty when nothing matches
- `action_string` — Sway entry produces `focus-window:sway:<con_id>`
- `action_string` — Hyprland entry produces `focus-window:hyprland:<address>`
- `action_string` — X11 entry produces `focus-window:x11:<window_id>`
- `cap_at_fifty` — more than 50 entries truncated to 50
- `backend_detection_prefers_sway_over_hyprland` — both env vars set → Sway wins

### Integration notes

Backends requiring a live compositor or OS (wlr, x11rb, Sway/Hyprland IPC, NSWorkspace, EnumWindows) are verified by manual smoke-testing on each platform. No automated integration tests.
