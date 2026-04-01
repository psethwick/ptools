# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Raycast-compatible launcher clone built with Rust + egui. Aims to run official Raycast extensions (JavaScript/TypeScript) via a QuickJS runtime embedded in Rust, with a native egui UI instead of a WebView.

## Commands

```bash
# Build and run
cargo run

# Build only
cargo build

# Run tests (unit tests live in src/extension_trait.rs and inline in other modules)
cargo nextest run

# Run a specific test
cargo nextest run <test_name>

# Lint
cargo clippy

# Format
cargo fmt
```

**Rust toolchain**: 1.91 (set in `rust-toolchain.toml`). Uses Rust 2024 edition.

**macOS feature flag**: The `objc` dependency and vibrancy code are gated behind the `macos` feature. Build with `cargo run --features macos` on macOS.

**Linux Wayland**: Global hotkey (`Alt+Space`) requires XWayland. Without it, use the Unix socket fallback: `echo show | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/launcher.sock`

## Architecture

### Core Data Flow

1. User types in search box → `App::trigger_search()` broadcasts via `ExtensionManager`
2. Each extension's `on_search()` runs asynchronously (Tokio runtime in `App`)
3. Results are sent back via `crossbeam-channel` as `ExtensionMessage::SearchResults`
4. `App::handle_messages()` receives results each frame and calls `List::set_items()`
5. egui re-renders with updated items

### Key Source Files

| File | Purpose |
|------|---------|
| `src/app.rs` | Main app struct, egui `App` impl, input handling, action routing |
| `src/extension_trait.rs` | `Extension` trait, `ExtensionItem`, `ExtensionMetadata` |
| `src/extension_manager.rs` | Loads/stores extensions, broadcasts searches, routes actions |
| `src/js_extension.rs` | QuickJS runtime wrapper for JS/TS extensions |
| `src/components.rs` | `List`, `ActionPanel`, `Detail`, `ToastManager` egui widgets |
| `src/clipboard_manager.rs` | Polls clipboard every 500ms, stores history |
| `src/hotkey_manager.rs` | Unix socket listener (Wayland fallback for window toggling) |
| `src/core_extensions/` | Built-in Rust extensions: app launcher, calculator, clipboard history |
| `extensions/` | Development JS/TS extension files for local testing |
| `specs/` | Design specs: UI layout, navigation, extension API, theming, etc. |

### Extension System

All extensions implement `src/extension_trait.rs::Extension` (async trait):
- `on_search(&str) -> Vec<ExtensionItem>` — called on every keystroke
- `on_action(&str, Option<&str>)` — called when user presses Enter on an item
- `initialize()` / `cleanup()` — lifecycle hooks

**JS/TS extensions** (`JsExtension`): One QuickJS `Runtime` + `Context` per extension, created at load time and reused. TypeScript is transpiled at load time via `oxc_transformer` (no Node.js needed). Extensions use a `raycast.updateList(items)` global to return results. A `fetch` binding and `console.log` are provided.

**Rust extensions**: Implement the `Extension` trait directly. Registered by `ExtensionManager::load_builtin_extensions()`.

**Extension discovery**:
1. Scans `~/.raycast-clone/extensions/` for user extensions
2. Falls back to `./extensions/` (repo root) for development
3. Built-in Rust extensions always loaded

**Extension metadata**: Each extension can have a companion `extension.json`. Key field: `"auto_load": false` means the extension only appears when its mode is active (not in global search).

### Action Routing

`App::execute_action()` parses action strings by prefix:
- `enter-mode:<name>` — activates a dedicated extension mode (scoped search)
- `launch-app:<path>` — platform-specific app launch
- `open-url:<url>` / `open-file:<path>` — platform open
- `calculator-result:<val>` / `calculator-copy:<expr>` — clipboard copy
- `clipboard-paste:<index>` — paste from clipboard history
- `<ext-name>:<action>:<id>` — routed to the named extension's `on_action()`

### Mode System

`App.current_mode: Option<String>` restricts search to a single extension. Ctrl+Shift+C toggles clipboard-history mode. Escape exits the current mode. A mode badge is shown below the search bar when active.

### UI Layout

Window is 600×400, non-resizable, always-on-top. The central panel has:
1. Full-width search box (always focused when `search_focused = true`)
2. Optional mode badge
3. Results list (left) + optional detail panel (right, 320px wide) side-by-side

Keyboard shortcuts: Arrow keys / Ctrl+N/P for navigation, Enter to execute, Ctrl+K for action panel, Escape to clear/hide.

## Development Workflow

### TDD — Red/Green/Refactor

When asked to fix a bug or add a feature, follow this cycle strictly:

1. **Red** — write a failing test that captures the desired behaviour. Confirm it fails before writing any implementation code.
2. **Green** — write the minimal implementation that makes the test pass.
3. **Refactor** — clean up the implementation and tests (naming, duplication, visibility) without changing behaviour. Re-run tests to confirm they still pass.

Prefer testing pure logic extracted into free functions or small state-machine methods (e.g. `reclaim_focus_on_text_input`, `take_activated_action`) over integration tests that require a full egui context. Tests live inline in `#[cfg(test)]` modules in the same file as the code under test.

### Clippy

`cargo clippy` must produce zero warnings. All new code must be clippy-clean before being considered done. Run `cargo clippy` after every implementation step and fix any warnings before moving on.
