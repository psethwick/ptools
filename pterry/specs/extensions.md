# Extension Architecture & Pipeline

## 1. Unified Extension Trait

All extensions implement a common `Extension` trait regardless of implementation language:

```rust
#[async_trait]
pub trait Extension: Send + Sync + Debug {
    fn metadata(&self) -> &ExtensionMetadata;
    async fn initialize(&mut self) -> Result<(), ExtensionError>;
    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError>;
    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError>;
    async fn cleanup(&self) -> Result<(), ExtensionError>;
    fn launcher_item(&self) -> Option<ExtensionItem>; // for non-auto-load extensions
}
```

## 2. Language Support

### JavaScript/TypeScript Extensions (`src/js_extension.rs`)

- Runtime: QuickJS via `rquickjs` crate
- Transpilation: `oxc_transformer` — strips TS types + JSX at load time, once
- File extensions: `.js`, `.ts`, `.tsx`
- Runtime lifecycle: one `Runtime` + `Context` per extension, reused across all searches
- Async: Promise pump loop runs after each JS call to resolve `async`/`await`
- `fetch`: native Rust binding into QuickJS, gated behind `"network"` permission

### Rust Extensions (`src/core_extensions/*.rs`)

- Implement `Extension` trait directly, compiled in
- Built-ins: app launcher, calculator, clipboard history, store, settings

## 3. Extension Manager (`src/extension_manager.rs`)

- Async extension loading and initialization
- Message-based communication via `crossbeam-channel`
- Broadcasts search to all `auto_load: true` extensions simultaneously
- Routes actions to the named extension's `on_action()`
- `auto_load: false` extensions only run when their mode is active

## 4. Extension Metadata

Extensions provide metadata via `extension.json`:

```json
{
    "name": "my-extension",
    "version": "1.0.0",
    "description": "Does something useful",
    "author": "username",
    "language": "typescript",
    "entry_point": "index.ts",
    "permissions": ["network", "clipboard"],
    "auto_load": true
}
```

Permissions: `"network"` (enables `fetch`), `"clipboard"` (enables clipboard read/write).

## 5. Multiple Commands per Extension

Real Raycast extensions define multiple commands in `package.json`, each with its own entry point. The metadata system supports this via an optional `commands` array:

```json
{
    "name": "my-extension",
    "commands": [
        { "name": "search", "title": "Search Items", "entry_point": "src/search.tsx" },
        { "name": "create", "title": "Create Item",  "entry_point": "src/create.tsx" }
    ]
}
```

When `commands` is present, the extension manager registers one `JsExtension` per command, each with a synthesised name `"<extension>:<command>"`. Each command appears as a separate launcher item. Single-file extensions without `commands` continue to work as before.

---

## Transpilation Pipeline

### Transpilation Strategy

Rust-native TypeScript transpilation via `oxc_transformer` (`src/transpiler.rs`):

- Transpiles TypeScript (+ JSX) → JavaScript once at load time
- No external Node.js dependency
- Two paths:
  - **Plain TS/JS** (`transpile_typescript`): strips types only, no JSX transform
  - **Raycast-style TSX** (`transpile_for_raycast`): strips types + JSX automatic runtime pointing at `@raycast/api/jsx-runtime`, then text-based ESM→CJS pass

Detection: `is_raycast_api_extension()` checks for `@raycast/api` imports and routes to the CJS path.

### ESM→CJS Conversion

`convert_esm_to_cjs()` in `src/transpiler.rs` converts oxc codegen output to CommonJS so it can be `eval()`'d in QuickJS. Handles:

- Named imports: `import { X } from 'mod'` → `const { X } = require("mod")`
- Default imports, namespace imports, mixed imports
- `export default`, `export function/class/const/let/var`

Known limitation: processes line-by-line, so multi-line import statements emitted by oxc codegen are not handled. Future fix: replace with a proper oxc CJS transform pass once available.

### Loading Process

1. **Discovery**: scans `~/.raycast-clone/extensions/` then `./extensions/` (dev fallback)
2. **Metadata loading**: reads `extension.json` or infers from file extension
3. **Transpilation**: TypeScript/TSX transpiled at load time
4. **Runtime creation**: `JsExtension::new_with_sender_and_clipboard()` creates one QuickJS context
5. **Initialization**: `extension.initialize()` called asynchronously
6. **Registration**: stored in `ExtensionManager` for search broadcasting
