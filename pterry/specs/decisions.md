# Technical Decisions & Known Gaps

## Architectural Decisions

### Trait-Based Architecture

- Language agnostic: same API for JS/TS and Rust extensions
- Compile-time type safety for the extension interface
- Async by design: non-blocking extension execution via Tokio
- Testable: extensions can be mocked in unit tests

### QuickJS vs V8/SpiderMonkey

QuickJS chosen for: small binary size, no external deps, embeddable in Rust via `rquickjs`, adequate ES2020 support. Trade-off: slower than V8 for CPU-heavy extensions; no JIT. Acceptable because extensions are I/O-bound (network, filesystem) rather than compute-bound.

### Immediate-Mode UI

egui re-renders every frame. Advantages: simple state model, no stale-view bugs. Trade-off: all item lists must be cheap to traverse; avoid cloning large vecs per frame.

## Known Gaps (not yet implemented)

- `Action.Push` / `useNavigation` — spec in [navigation.md](navigation.md)
- `Grid` component rendering
- `console.error/.warn/.info/.debug` — spec in [raycast-shim.md](raycast-shim.md)
- `@raycast/utils` shim — spec in [raycast-shim.md](raycast-shim.md)
- Node.js built-in stubs (`path`, `os`, `crypto`, …) — spec in [raycast-shim.md](raycast-shim.md)
- `getPreferenceValues()` — spec in [preferences.md](preferences.md)
- `LocalStorage` API — spec in [runtime.md](runtime.md)
- Per-extension global hotkeys — spec in [os-integration.md](os-integration.md)
- Multiple commands per extension — spec in [extensions.md](extensions.md)
- URL/Base64 icon fetching — spec in [icons.md](icons.md)
- Multi-line import handling in ESM→CJS converter — spec in [extensions.md](extensions.md)

## Future Enhancements

- **Hot reloading**: watch `~/.raycast-clone/extensions/` with `notify` crate; reload changed extensions without restart
- **`MenuBarExtra`**: tray icon component; requires a second OS window/tray handle separate from the launcher window
- **`AI.ask()`**: proxy to a configurable LLM endpoint; gated behind an `"ai"` permission
- **Frequency/recency ranking**: track item selection counts in `~/.raycast-clone/usage.json`; boost frequently-used items in app launcher and store results
