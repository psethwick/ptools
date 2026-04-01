# Icon / Image Resolution

Icons appear on `List.Item`, `Grid.Item`, store listings, and in action panels. The `icon` field on `ExtensionItem` is a string; resolution order:

1. **Emoji / short string** (`chars().count() <= 2`): rendered as text in a square frame
2. **Named Raycast icon** (matches a key in the `Icon` map, e.g. `"Globe"`): resolved to the emoji value in the map
3. **`file://` or absolute path**: read from disk as image bytes, decoded, uploaded as a GPU texture
4. **`http://` / `https://` URL**: fetched asynchronously (requires `"network"` permission or an allowlist for known CDNs); cached in `~/.raycast-clone/icon_cache/` by URL hash; displayed once loaded, placeholder shown until then
5. **`data:image/...;base64,...`**: decoded from Base64, uploaded as texture immediately

Texture cache in `List` is bounded to 256 entries (LRU eviction) to prevent GPU memory leaks.
