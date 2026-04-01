# Raycast Clone — Spec Index

A Raycast-compatible launcher clone. Runs official Raycast extensions (JavaScript/TypeScript) via an embedded QuickJS runtime, with a native egui UI instead of a WebView.

## Core Technical Stack

- **UI Framework**: egui (immediate-mode, no WebView)
- **Scripting Runtime**: rquickjs (QuickJS wrapper for Rust)
- **Windowing**: winit + window-vibrancy (macOS frosted-glass, Windows acrylic)
- **TypeScript transpilation**: oxc_transformer (Rust-native, no Node.js dependency)
- **Async**: Tokio runtime for extension tasks; crossbeam-channel for UI↔extension messaging

## Spec Files

| File | Contents |
|------|----------|
| [extensions.md](extensions.md) | Extension trait, JS/TS + Rust extensions, metadata, multiple commands, transpilation pipeline, loading process |
| [communication.md](communication.md) | Message system, search flow, action flow |
| [ui.md](ui.md) | Window layout, List, ActionPanel, Detail, Form, Grid, Toast |
| [os-integration.md](os-integration.md) | Window behaviour, global hotkeys, platform effects, clipboard |
| [theming.md](theming.md) | Theme options, colour tokens, implementation |
| [store.md](store.md) | Extension store UI, own registry, Raycast store integration, install state |
| [raycast-shim.md](raycast-shim.md) | @raycast/api shim, component/hook/utility inventory, console API, @raycast/utils, Node.js stubs |
| [navigation.md](navigation.md) | Action.Push / useNavigation stack |
| [runtime.md](runtime.md) | `environment` object, LocalStorage API, extension error handling |
| [preferences.md](preferences.md) | Per-extension preference schema, storage, settings UI, getPreferenceValues() |
| [icons.md](icons.md) | Icon/image resolution order, texture cache |
| [cli.md](cli.md) | CLI flags (--extension, --show, --query) |
| [window-switcher.md](window-switcher.md) | Window switcher extension: backend detection chain (Sway, Hyprland, wlr-foreign-toplevel, x11rb, macOS, Win32), action protocol, tests |
| [decisions.md](decisions.md) | Architectural trade-offs and known gaps |
