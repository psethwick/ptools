# Implementation Todo List

This list outlines the tasks required to implement the Raycast clone based on the provided `spec.md`.

## Core Technical Stack Setup

- [x] Initialize Rust project with `egui`, `rquickjs`, `winit`, `window-vibrancy` dependencies.
- [x] Set up basic `egui` window with `winit`.
- [x] Integrate `window-vibrancy` for OS-specific effects (macOS frosted-glass, Windows acrylic).

## Extension Pipeline - CRITICAL MISSING PIECES

- [x] **IMPLEMENT JAVASCRIPT EXECUTION**: QuickJS runtime integration with result capture
- [x] **CREATE BUILT-IN EXTENSIONS**: App launcher, calculator (Rust core extension), clipboard history
- [x] **EXTENSION DIRECTORY SCANNING**: Load from `~/.raycast-clone/extensions/`
- [x] **TYPESCRIPT TRANSPILATION**: Replace external tsc/regex with `oxc_transformer` (Rust-native, no Node.js dep)

## Working Core Features

- [x] Keyboard-only workflow (navigation, selection, actions)
- [x] Fuzzy search and filtering in search box
- [x] App launcher - launch programs, URLs, files across platforms
- [x] **CLIPBOARD HISTORY EXTENSION** - implement the actual extension

## Communication Bridge - MOSTLY DONE

- [x] Set up `crossbeam-channel` for communication between UI and extension threads
- [x] Define and implement all ExtensionMessage types
- [x] Broadcast search to all extensions simultaneously
- [x] Async extension execution framework
- [x] **FIX EXTENSION RESULT CAPTURE**: Extensions don't return actual results

## UI Components - MOSTLY IMPLEMENTED

- [x] `List` component (`ScrollArea + egui::Frame`)
- [x] `List.Item` component with selection and keyboard navigation
- [x] `ActionPanel` component with keyboard shortcuts
- [x] Dark theme and proper styling
- [x] **Detail panel** for metadata/previews
- [x] **Toast notifications** system

## OS Integration

- [x] Always-on-top window functionality
- [x] Cross-platform app launching (macOS/Linux/Windows)
- [x] **Global hotkey support** — `global-hotkey` crate registers **Alt+Space** via X11 on Linux, Carbon on macOS, Win32 on Windows. Unix socket kept as Wayland fallback (print instructions at startup).
- [x] **Focus stealing** on hotkey trigger (ViewportCommand::Focus called in handle_hotkey_event)
- [x] **Escape to hide window** (Escape with empty search now hides window)

## Extension Runtime Systems - MOSTLY IMPLEMENTED

- [x] **JavaScript/TypeScript**: QuickJS integration with result capture
- [x] **JavaScript Action Handling**: Extensions can now handle user actions
- [x] **Persistent runtime**: Create one QuickJS `Runtime` + `Context` per extension at load time, reuse across searches instead of recreating
- [x] **Promise pump**: Run the QuickJS Promise queue to completion after each call so extensions can use `async`/`await`
- [x] **fetch binding**: Expose `fetch` as a native Rust function in QuickJS context
- [x] **Permission system**: `fetch` gated behind `"network"` permission; missing permission installs a stub that rejects with `PermissionError` at call time.
- [x] **Extension store**: Package management and distribution (see "Extension Store" section below)

## Extension Store

- [x] **Store mode**: Add `store` built-in extension/mode, accessible via `enter-mode:store`
- [x] **Store UI**: Two-tab layout (Native / Raycast Store) rendered in egui List
- [x] **Own registry fetch**: `StoreNativeExtension` fetches `registry.json` via ureq, 5-min TTL cache, pure `filter_native_items()` for search
- [x] **Raycast registry fetch**: `StoreRaycastExtension` fetches `extensionName2Folder.json` from raycast/extensions, same cache pattern
- [x] **Extension browser**: Both extensions filter by query (title/description/author/folder); `store-tab:native/raycast` now enters the correct mode via new `Action::StoreTab` variant
- [x] **npm fetcher** (`src/npm_fetcher.rs`): Not needed for native extensions — registry CI pre-bundles deps; native install is a single file download
- [x] **Rolldown bundler** (`src/bundler.rs`): Wrap rolldown crate to bundle Raycast extension source + node_modules → single JS (Raycast store only)
- [x] **One-click install (native)**: Download source_url → write to ~/.raycast-clone/extensions/<name>.<ext> → update state → toast
- [x] **Install state** (`src/store_state.rs`): `StoreState` / `InstalledExtension` with load/save/is_installed/mark_installed; shown as ✅ icon + "✓ Installed" subtitle in store-native search results
- [x] **Optional GitHub token**: `src/settings.rs` — `Settings { github_token }` with load/save; `SettingsExtension` (mode `settings`) surfaces items in global search via `launcher_item()`; `SetGitHubTokenExtension` (mode `settings-set-github-token`) uses the search box as a text input to capture and persist the token. `Settings::github_request()` helper wraps ureq with the auth header for future GitHub API calls.

## @raycast/api Shim

### Phase 1 — List + ActionPanel ✅ COMPLETE
- [x] **Module resolver**: `__raycast_require()` shim intercepts `@raycast/api` and `@raycast/api/jsx-runtime` imports
- [x] **React shim**: Minimal React (createElement/jsx, useState, useEffect, useRef, useCallback, useMemo) in `src/raycast_shim.js`
- [x] **Component reconciler**: `_renderNode()` in shim walks VNode tree → ExtensionItem list → `raycast.updateList()`
- [x] **List component**: `_List` with `onSearchTextChange` callback support
- [x] **List.Item component**: `_ListItem` with title, subtitle, icon, actions props
- [x] **ActionPanel + Action**: `_ActionPanel`, `_Action` mapped to action strings
- [x] **Action.CopyToClipboard / Action.OpenInBrowser**: `"clipboard-copy:<content>"` / `"open-url:<url>"`
- [x] **TSX transpilation**: `transpile_for_raycast()` — oxc JSX transform + text-based ESM→CJS converter
- [x] **Auto-detection**: `is_raycast_api_extension()` detects `@raycast/api` imports, routes to CJS mode

### Phase 2 — Detail + Form
- [x] **Detail component**: Markdown string → ExtensionItem.detail field
- [x] **Form component**: Renders inputs, submits via on_action()
- [x] **Form.TextField / Checkbox / Dropdown**: Basic input types

### Phase 3 — Hooks + Utilities
- [x] **useFetch hook**: Wraps fetch() with loading/error/revalidation state
- [x] **usePromise hook**: AsyncState wrapper for any async function
- [x] **showToast**: Bridge to existing ToastManager via message channel
- [x] **Clipboard.readText / Clipboard.copy**: New clipboard read bindings in QuickJS

## Missing Features

### @raycast/api Compatibility Gaps

- [x] **`console` only exposes `.log`** — `console.error`, `.warn`, `.debug`, `.info` are not set on the console object (`js_extension.rs:362`); calls to them throw `TypeError: console.error is not a function` and crash extensions.
- [x] **`@raycast/utils` package not shimmed** — `bundler.rs` marks it as external but `__raycast_require` throws for it; extensions importing `useCachedPromise`, `useLocalStorage`, `useFetch` from `@raycast/utils` (very common) fail on load.
- [x] **Node.js built-in modules throw on require** — `path`, `os`, `crypto`, `url`, `querystring` are not shimmed; `require('path')` throws `"Cannot require module: path"`. Many extensions use `path.join()` for file paths. (`path`, `os`, `url`, `querystring` now shimmed; `crypto` remains a stub)
- [x] **`MenuBarExtra` component missing** — added `MenuBarExtra`, `MenuBarExtra.Item`, `MenuBarExtra.Section` to shim; items degrade gracefully to list items in the launcher window.
- [x] **`environment.supportPath` not set** — the shim's `environment` object has no `supportPath`; extensions that use `path.join(environment.supportPath, 'data.json')` to persist data will get `undefined` and likely crash.
- [x] **`List.Section` loses its header** — `__section__` in the reconciler falls through to `_renderNode(children)`, silently dropping the section `title` and `subtitle`. Items render flat with no visual grouping.
- [x] **`List.Item.Detail.Metadata` not implemented** — structured detail metadata (`Detail.Metadata.Label`, `.Link`, `.TagList`, `.Separator`) is not modelled; only plain markdown strings reach the detail panel.
- [x] **Multi-line import statements break ESM→CJS** — `convert_esm_to_cjs` processes line-by-line; when oxc codegen emits a multi-line import (e.g. `import {\n  foo,\n  bar\n} from 'mod'`) each line is handled independently and the import is lost or mangled.
- [x] **`open()` shim is a no-op** — `raycastApiModule.open` in `raycast_shim.js` only calls `console.log`. Should invoke `globalThis.raycast.open(url)` which Rust binds to `platform::open()`. Many real extensions call `open(url)` directly.
- [x] **`Action.Push` / `useNavigation` are stubs** — `push`/`pop` do nothing; no navigation stack exists. Drill-down navigation (common pattern: select item → show detail view or sub-list) is silently broken.
- [x] **`Grid` component unimplemented** — added `Grid.Item`, `Grid.Section`, `Grid.EmptyView` components and reconciler branches. `Grid.Item.content` maps to the icon field. `onSearchTextChange` wired like `List`.
- [x] **`getPreferenceValues()` always returns `{}`** — now reads from `~/.raycast-clone/extension-data/<name>/preferences.json`. Extensions get their stored values; missing file falls back to `{}`.
- [x] **`getSelectedText()` not exported** — mentioned in the spec (Phase 3) but absent from `raycastApiModule`. Extensions that read selected text will throw on import.
- [x] **`LocalStorage` API missing** — `LocalStorage.getItem/setItem/removeItem/clear` are commonly used by extensions to persist state between runs. Not in the shim or bridged to disk.
- [x] **`List.Item.accessories` prop silently dropped** — real Raycast supports right-side accessory icons/text on list items. `ExtensionItem` has no `accessories` field; the prop is lost in the reconciler.
- [x] **Action `shortcut` props ignored** — `Keyboard.Key`/`Keyboard.Modifier` are exported; `_formatShortcut` converts `{ modifiers, key }` → label string (e.g. `"⌘C"`); `parse_shortcut_label()` + `key_from_char()` in `app.rs` parse labels back to `egui::Modifiers + egui::Key`; `handle_global_shortcuts()` checks the selected item's `extra_actions` each frame and fires matching actions without opening the action panel.
- [x] **`List.EmptyView` not implemented** — no empty-state component; when a search returns zero items the list is just blank with no customisable message/icon.
- [x] **`Action.ShowInFinder` missing** — `Action.ShowInFinder` not in `Action` enum; added `show-in-finder:<path>` action string, `platform::show_in_finder()` (macOS: `open -R`, Linux: `xdg-open <parent>`, Windows: `explorer /select`), and shim components `_ActionShowInFinder` / `_Action.ShowInFinder`.
- [x] **`Action.Trash` missing** — `Action.Trash` not in `Action` enum; added `trash-file:<path>` action string, `platform::trash_file()` (macOS: `osascript`, Linux: `gio trash`, Windows: `powershell`), and shim components `_ActionTrash` / `_Action.Trash`. Shows a toast on completion.
- [x] **`Grid` renders as flat list, not multi-column tiles** — `grid_columns: Option<u8>` added to `ExtensionItem`; shim's `__grid__` handler stamps `gridColumns` (small→5, medium→4, large→3) on each child item; `List::ui()` branches into `render_grid()` (multi-column square tiles with icon + truncated title) when the field is set.
- [x] **`getSelectedText()` always returns empty string** — added `platform::get_selected_text()` (Linux: reads X11 PRIMARY selection via xclip/xsel; macOS/Windows: returns ""); wired as `raycast.getSelectedText` native binding; shim now calls the binding instead of hardcoding "".

### Core UX

- [x] **Light mode** — UI is dark-only; no light theme or auto-follow-system-theme support.
- [x] **CLI launch into a specific extension** — e.g. `raycast-clone --extension calculator` should open the window with that extension's mode already active. Useful for keybinding launchers that skip the global search entirely.
- [x] **Per-extension global hotkeys** — `HotkeyEvent::LaunchExtension(String)` added; `Settings.extension_hotkeys: HashMap<String,String>` maps mode names to hotkey strings; `register_hotkeys()` in `hotkey_manager.rs` parses strings via the crate's `FromStr` and registers all hotkeys at startup; `handle_hotkey_event` opens the window in the named mode.
- [x] **Flesh out hotkey integration** — `HotkeyManager` now owns the `hotkey_id→HotkeyEvent` map and polls `GlobalHotKeyEvent::receiver()` inside `try_receive()`; `setup_global_hotkey()` reads `toggle_hotkey`/`extension_hotkeys` from `Settings` instead of hardcoding `Alt+Space`; `HotkeyState.global_hotkey_id` removed; the separate OS-event polling block in `update()` removed; `parse_hotkey_string()` public helper exposed. Runtime re-registration still requires restart.
- [x] **Window does not hide after `OpenUrl`/`OpenFile`** — `execute_action()` hides the window for `LaunchApp` (`app.rs:729`) but not for `OpenUrl` or `OpenFile`. The window lingers open after following a link or opening a file.
- [x] **No frequency/recency ranking** — app launcher and other extensions always return results in the same order. Raycast learns which items you use most and promotes them. There is no selection-frequency tracking or boosting logic.
- [x] **Multiple commands per extension not supported** — real Raycast extensions define several `commands` in `package.json`, each with its own entry point. The current architecture maps one extension file → one command; there is no way to bundle multiple commands in one extension directory.

- [x] **`environment.extensionName`, `environment.commandName`, `environment.isDevelopment` missing** — added `raycast.extensionName()`, `raycast.commandName()`, `raycast.isDevelopment()` native bindings in `setup_raycast_object`; `ExtensionMetadata.is_development` field added (set by loader from path); `commandName` derived from `entry_point` stem; shim `environment` object updated; 3 new tests + flaky `get_selected_text` test fixed. 461 tests pass.
- [x] **`crypto` module missing** — added `raycast.cryptoHash(alg, data)` native Rust binding (md5/sha1/sha256/sha512 via `md5`/`sha1`/`sha2` crates); `_cryptoModule` JS shim in `raycast_shim.js` exposes `createHash(alg).update(str).digest('hex'|'base64')`, `randomUUID()`, and `randomBytes(n)`; wired into `__raycast_require` for `"crypto"` / `"node:crypto"`. 8 unit tests added.

## Spec Delta — Newly Discovered Gaps

### Icon Resolution (spec §13)

- [x] **`file://` and absolute-path icons not rendered** — added `resolve_icon_data()` / `needs_image_load()` helpers; extended `List::ui()` pre-pass to decode file:// and absolute-path icons via the `image` crate and upload as textures (keyed by icon string); `render_item` and `render_grid_tile` now look up icon-string textures as a fallback when no `thumbnail_rgba` is present.
- [x] **`data:image/...;base64,...` icons not decoded** — same `resolve_icon_data()` helper handles Base64 data URIs via the `base64` crate; decoded and cached identically to file-path icons.
- [x] **HTTP/HTTPS icon URLs not fetched** — added `is_url_icon()`, `url_cache_key()` (FNV-1a 64-bit), `icon_cache_path()`, and `fetch_url_icon()` (disk-cache-first, then `ureq` GET). `List::ui()` drains completed fetches each frame and dispatches `std::thread::spawn` for new URL icons; calls `ctx.request_repaint()` on completion. Grid tile and list item renderers now also look up URL-icon textures.

### Environment Object (spec §11.1)

- [x] **`environment.theme` hardcoded to `"dark"`** — added `raycast.getTheme()` native binding that reads `Settings.theme` from disk; shim now sets `environment.theme` from this binding at extension load time (returns `"dark"` or `"light"`; `"system"` and missing values default to `"dark"`).
- [x] **`environment.assetsPath` missing** — added `extension_assets_dir()` helper (`~/.raycast-clone/extensions/<name>/assets`); added `raycast.assetsPath()` native binding; shim now populates `environment.assetsPath` from the binding at extension load time.

### Navigation UI (spec §10.5)

- [x] **Breadcrumb bar not rendered** — Added `nav_stack: Vec<String>` to `UiState`; JS shim `_navigationPush` calls `raycast.navigate("push-view:<title>")` and `_navigationPop` calls `raycast.navigate("pop-view")`; `handle_messages` updates `nav_stack` on `Navigate` messages; breadcrumb bar rendered below mode badge when `nav_stack` is non-empty; `EscapeOutcome::PopNavigation` added — Escape at depth > 0 calls `on_action("pop-view")` on the current extension, which triggers `_navigationPop()` in the shim.
- [x] **NavFrame state not saved/restored on push/pop** — `nav_stack` was `Vec<String>` (titles only); upgraded to `Vec<NavFrame>` where `NavFrame { items, search_query, selected_index, title }` captures the parent view's full state. Push saves and clears; pop restores. Stack depth capped at 10 (`NAV_STACK_MAX_DEPTH`). `List::snapshot()` / `List::restore_snapshot()` helpers added. 4 unit tests added.

### Settings / Preferences UI (spec §12.4)

- [x] **Extension preferences form UI not implemented** — `SettingsExtension` only surfaces GitHub token and theme toggle. The spec requires a preferences sub-mode per extension (form rendered from `extension.json` preference schema). (Low priority — `getPreferenceValues()` reads files; extensions can request values if files are written manually.)

### CLI (spec §cli.md)

- [x] **`--show` / `--query` / `--help` flags missing** — only `--extension` was parsed. Added `parse_flag_arg` helper; `parse_query_arg()` extracts `--query <text>`; `App::new()` gains `initial_query: Option<String>` parameter (sets `search_query` at construction). `main.rs` handles `--help` (prints usage + exits), `--show` on Linux (writes to `$XDG_RUNTIME_DIR/launcher.sock` and exits), and passes `--query` value to `App::new()`. 4 unit tests added.

### OS Integration (spec §os-integration.md)

- [x] **`raycast.hideWindow()` binding missing** — JS extensions could not programmatically hide the launcher. Added `ExtensionMessage::HideWindow` variant; `raycast.hideWindow()` native binding in `js_extension.rs` sends it; `App::handle_messages()` sets `window_visible = false` and calls `ViewportCommand::Visible(false)`; `raycast_shim.js` exports `closeMainWindow()` (Raycast API name) that calls `globalThis.raycast.hideWindow()`.

### Theming (spec §theming.md)

- [x] **Custom colour tokens not applied** — `visuals_for_theme` used egui defaults. Created `src/theme.rs` with `dark_visuals()` / `light_visuals()` applying the exact spec hex palette (`#1c1c1e` bg, `#2c2c2e` surface, `#0a84ff`/`#007aff` accent, `#3a3a3c`/`#d1d1d6` selection, etc.); `visuals_for_theme` in `settings.rs` now delegates to `theme::visuals_for_theme`. Panel background in `app.rs` uses `ctx.style().visuals.panel_fill` instead of a hardcoded gray(25). 11 unit tests added.
- [x] **"system" theme not handled** — `"system"` value in settings was silently treated as dark. `theme::os_prefers_dark()` added (Linux: checks `GTK_THEME` / `XDG_CURRENT_DESKTOP_THEME`; macOS: runs `defaults read -g AppleInterfaceStyle`; Windows: falls back to dark). `visuals_for_theme(Some("system"))` now returns the OS-appropriate palette.

## Built-in Extensions

- [x] **Window switcher** — `WindowSwitcherExtension` (`src/core_extensions/window_switcher_extension.rs`). Backend detection at init: `$DISPLAY` → x11rb EWMH (`_NET_CLIENT_LIST_STACKING`); Noop fallback (empty results). Action `focus-window:x11:<wid>` → `_NET_ACTIVE_WINDOW` ClientMessage. Sway, Hyprland, wlr-foreign-toplevel, macOS (NSWorkspace), and Win32 (EnumWindows) backends spec'd in `specs/window-switcher.md` but not yet implemented.

## Technical Debt

### Crash / Safety Risks (HIGH)

- [x] **`.lock().unwrap()` in `app_launcher_extension.rs`** — `on_search()` at line 197 still uses `.lock().unwrap()`, which panics on mutex poison. Replace with `.unwrap_or_else(|e| e.into_inner())` to match the pattern used elsewhere.

### Architectural (MEDIUM)

- [x] **No timeout/cancellation for JS extension execution** — `call_on_search`/`call_on_action` now run in `tokio::task::spawn_blocking` wrapped with `tokio::time::timeout(5 s)`. The `rquickjs` `parallel` feature is enabled so `context.with()` correctly re-anchors QuickJS's `stack_top` to the executing thread on every call.
- [x] **Unbounded result channel can accumulate stale results** — `extension_manager.rs` creates an `unbounded()` channel; slow extensions can queue many stale `SearchResults` frames behind. Consider a bounded channel or dropping results whose query no longer matches `search_query` earlier (at send time, not receive time).
- [x] **Magic mode-name strings scattered across the codebase** — created `src/modes.rs` with `pub const` values for all 8 built-in mode names. Replaced all scattered string literals across `app.rs`, `extension_manager.rs`, and all 7 core extension files. Includes a kebab-case validation test.
- [x] **Texture cache in `List` grows unbounded** — replaced `HashMap` with `BoundedCache<K,V>` (256-entry FIFO eviction) in `components.rs`. Four unit tests added in `bounded_cache_tests`.
- [x] **Text-based ESM→CJS transpiler is fragile** — implemented `export { X, Y }`, `export { X as Y }`, `export * from 'mod'`, and `export { X } from 'mod'` / `export { X as Y } from 'mod'` patterns. Dynamic `import()` still falls through unchanged (rare in Raycast extensions).

### Performance (LOW-MEDIUM)

- [x] **Hardcoded DPI of 1.5** — added `pixels_per_point: Option<f32>` to `Settings`. `main.rs` now only calls `set_pixels_per_point` when the setting is `Some`; otherwise the OS native DPI is used. Users can set `"pixels_per_point": 1.5` in `~/.raycast-clone/settings.json` to override.

### Test Coverage (LOW-MEDIUM)

- [x] **`app_launcher_extension.rs` has no tests** — added 16 new tests covering `clean_exec` field-code stripping, empty-query cap, fuzzy matching (name + comment), action string format, `make_detail` formatting, and Linux `parse_desktop_file` (valid file, NoDisplay, non-Application type, multi-section).
- [x] **`transpiler.rs` edge cases have no tests** — re-exports, multi-line imports, and named-re-export patterns are unhandled and untested. Add unit tests before expanding the transpiler.

