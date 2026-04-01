# Extension Store

## Store UI

Accessible via `enter-mode:store`. Two-tab layout (Native / Raycast Store) rendered as a `List`.

## Own Registry

- GitHub repo hosting `registry.json` (array of extension manifests)
- Each entry: `name`, `title`, `description`, `author`, `version`, `source_url`, `icon`, `permissions`
- Single-file JS/TS; deps pre-bundled by CI
- Fetched at store-open time with a 5-minute TTL cache
- One-click install: download `source_url` → write to `~/.raycast-clone/extensions/<name>.js` → toast

## Raycast Store Integration

- Extension list from `extensionName2Folder.json` in the raycast/extensions repo
- Install flow:
  1. Download source from GitHub API
  2. Parse `package.json` for deps
  3. Fetch npm tarballs via `ureq` (no npm CLI needed)
  4. Bundle with `rolldown` (Rust crate) into a single `.js`
  5. Write to `~/.raycast-clone/extensions/<name>/`
- Optional GitHub token (stored in settings) raises rate limit from 60 to 5,000 req/hr

## Install State

Tracked in `~/.raycast-clone/store_state.json`:
```json
{ "installed": [{ "name": "...", "version": "...", "source": "raycast|native", "installed_at": "..." }] }
```
