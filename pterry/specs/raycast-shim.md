# @raycast/api Shim

Real Raycast extensions import from `@raycast/api` and render React JSX. The shim (`src/raycast_shim.js`) is injected into each QuickJS context before the extension code runs.

## Module Resolver

`__raycast_require(name)` is set as the global `require`. Intercepts:

- `@raycast/api` → returns the full shim module object
- `@raycast/api/jsx-runtime` → returns `{ jsx, jsxs, Fragment }`
- `react`, `react/jsx-runtime` → same as above (React compat)
- `@raycast/utils` → returns the `@raycast/utils` shim (see below)
- `@oxc-project/runtime/helpers/*`, `@babel/runtime/helpers/*` → `babelHelpers` passthrough
- Anything else → throws `"Cannot require module: <name>"`

## Component Inventory

| Component | Status | Notes |
|-----------|--------|-------|
| `List` | ✅ | `onSearchTextChange`, `isLoading` props |
| `List.Item` | ✅ | title, subtitle, icon, accessories, detail, actions |
| `List.Section` | ⚠️ | children flattened, header not rendered |
| `List.EmptyView` | ❌ | not implemented |
| `ActionPanel` | ✅ | |
| `Action` | ✅ | `onAction` callback |
| `Action.CopyToClipboard` | ✅ | |
| `Action.OpenInBrowser` | ✅ | |
| `Action.Push` | ❌ | stub — see [navigation.md](navigation.md) |
| `Action.ShowInFinder` | ❌ | |
| `Action.Trash` | ❌ | |
| `Detail` | ✅ | markdown string |
| `Detail.Metadata` | ❌ | not implemented |
| `Form` | ✅ | TextField, Checkbox, Dropdown |
| `Grid` | ❌ | stub, not rendered |
| `MenuBarExtra` | ❌ | out of scope for now |

## Hook Inventory

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ | |
| `useEffect` | ✅ | |
| `useRef` | ✅ | |
| `useCallback` | ✅ | identity (no memoisation) |
| `useMemo` | ✅ | eagerly evaluated |
| `useNavigation` | ❌ | stubs — see [navigation.md](navigation.md) |
| `useFetch` | ✅ | requires `"network"` permission |
| `usePromise` | ✅ | |
| `useLocalStorage` | ❌ | see [runtime.md](runtime.md) |

## Utilities

| Utility | Status | Notes |
|---------|--------|-------|
| `showToast` | ✅ | bridges to `ToastManager` |
| `open(url)` | ❌ | stub — calls `console.log` only |
| `getPreferenceValues()` | ❌ | returns `{}` — see [preferences.md](preferences.md) |
| `getSelectedText()` | ❌ | not exported |
| `Clipboard.readText` | ✅ | |
| `Clipboard.copy` | ✅ | |
| `Clipboard.paste` | ⚠️ | falls back to copy; no real paste |
| `environment` | ⚠️ | `isDevelopment`, `theme` set; `supportPath` missing — see [runtime.md](runtime.md) |
| `Icon` | ✅ | emoji map for named icons |
| `Color` | ✅ | colour token map |
| `Keyboard` | ✅ | constants exported; shortcut labels parsed and wired in `handle_global_shortcuts()` |

## console API

The `console` global in QuickJS must expose all standard methods. All route to `println!`:

| Method | Status |
|--------|--------|
| `console.log` | ✅ |
| `console.error` | ❌ — throws TypeError |
| `console.warn` | ❌ — throws TypeError |
| `console.info` | ❌ — throws TypeError |
| `console.debug` | ❌ — throws TypeError |

Fix: set all five methods on the console object in `setup_console()`.

## @raycast/utils Shim

Many extensions import from `@raycast/utils`. The bundler marks it external; `__raycast_require` must handle it. Minimal shim to add to `raycast_shim.js`:

| Export | Implementation |
|--------|---------------|
| `useCachedPromise` | alias for `usePromise` with local-storage-backed initial data |
| `useLocalStorage` | backed by `raycast.storageGet/storageSet` bindings (see [runtime.md](runtime.md)) |
| `useFetch` | alias for existing `useFetch` hook |
| `showFailureToast` | `showToast({ style: "failure", ... })` |
| `getAvatarIcon` | returns a generated emoji string |

## Node.js Built-in Stubs

Extensions frequently `require('path')`, `require('os')`, etc. Add stubs to `__raycast_require`:

| Module | Stub |
|--------|------|
| `path` | `join(...parts)`, `basename(p)`, `dirname(p)`, `extname(p)`, `resolve(...parts)` — pure JS string operations |
| `os` | `homedir()` → `raycast.homedir()` (Rust binding), `platform()` → `"linux"\|"darwin"\|"win32"`, `tmpdir()` |
| `url` | `URL` class (basic), `URLSearchParams` |
| `querystring` | `stringify(obj)`, `parse(str)` |
| `crypto` | `createHash(alg).update(str).digest('hex')` — basic MD5/SHA1/SHA256 via Rust binding |
