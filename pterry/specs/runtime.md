# Extension Runtime Environment

## `environment` Object

The shim's `environment` object must expose:

```js
environment = {
  isDevelopment: false,         // true only when loaded from ./extensions/ dev path
  theme: "dark" | "light",     // current UI theme
  textSize: "medium",
  launchType: "userInitiated",
  extensionName: "my-ext",     // from metadata.name
  commandName: "search",       // from metadata.entry_point basename
  supportPath: "/home/user/.raycast-clone/support/my-ext",  // writable per-extension dir
  assetsPath: "/home/user/.raycast-clone/extensions/my-ext/assets",
}
```

`supportPath` is created on first access if it doesn't exist. Rust exposes `environment.supportPath` as a string via a binding rather than letting JS construct it.

## LocalStorage API

Extensions use `LocalStorage` to persist small key/value data between runs. Backed by a per-extension JSON file at `<supportPath>/local_storage.json`.

```js
LocalStorage.setItem(key, value)  // value coerced to string; returns Promise<void>
LocalStorage.getItem(key)         // returns Promise<string | undefined>
LocalStorage.removeItem(key)      // returns Promise<void>
LocalStorage.clear()              // returns Promise<void>
LocalStorage.allItems()           // returns Promise<Record<string, string>>
```

Rust side: `raycast.storageGet(key)` / `raycast.storageSet(key, value)` / `raycast.storageDel(key)` / `raycast.storageClear()` native bindings; JS shim wraps them in `Promise.resolve()`.

## Extension Error Handling

When an extension errors during `on_search` or `on_action`:

1. The `ExtensionError` is caught at the Tokio task boundary
2. A `SearchResults` with zero items is returned (search errors) or `on_action` logs the error (action errors)
3. An `ExtensionMessage::ShowToast` is sent with `style: "failure"` and the error message
4. The extension remains loaded and continues to receive future searches (errors are not fatal)

JS exceptions during rendering are caught by the shim's try/catch in `_doRender()` and logged via `console.log`.
