# Per-Extension Preferences

## Overview

Real Raycast extensions declare a preference schema in `package.json` under `"preferences"`. Users configure values in the settings UI; extensions read them via `getPreferenceValues()`.

## Schema

Added to `extension.json` (or parsed from `package.json` for Raycast extensions):

```json
{
  "preferences": [
    { "name": "apiKey",   "type": "password", "title": "API Key",   "required": true },
    { "name": "maxItems", "type": "textfield","title": "Max Items", "default": "20" },
    { "name": "verbose",  "type": "checkbox", "title": "Verbose",   "default": false },
    { "name": "region",   "type": "dropdown", "title": "Region",
      "data": [{ "title": "US", "value": "us" }, { "title": "EU", "value": "eu" }],
      "default": "us" }
  ]
}
```

Types: `textfield`, `password` (masked input), `checkbox`, `dropdown`, `file`, `directory`.

## Storage

Preference values stored in `~/.raycast-clone/preferences/<extension-name>.json` as a flat `Record<string, string | boolean>`.

## Settings UI

`SettingsExtension` gains a preferences sub-mode: selecting an installed extension shows its preference fields as a `Form`. Submitting calls `Settings::save_preferences(extension, values)`.

## `getPreferenceValues()`

Returns the stored values merged with schema defaults:

```js
// In shim:
getPreferenceValues: function() {
  return JSON.parse(raycast.getPreferences()); // Rust binding reads from disk
}
```
