# UI Components

## Window Layout

Fixed 600×400, non-resizable, always-on-top. Layout from top to bottom:

Normal view:
```
┌─────────────────────────────────────────────────────┐
│  [search box — always focused]                      │
│  [mode badge — shown when a mode is active]         │
│  [nav breadcrumb — shown when nav depth > 1]        │
├──────────────────────────────┬──────────────────────┤
│  Results list (scrollable)   │  Detail panel        │
│                              │  (320 px, optional)  │
└──────────────────────────────┴──────────────────────┘
│  [Toast notifications — bottom overlay]             │
└─────────────────────────────────────────────────────┘
```

Form view (search bar, mode badge, breadcrumb, and list are all hidden):
```
┌─────────────────────────────────────────────────────┐
│  [Form fields — take over entire central panel]     │
│    field 1                                          │
│    field 2                                          │
│    …                                                │
│  [Submit button]                                    │
└─────────────────────────────────────────────────────┘
│  [Toast notifications — bottom overlay]             │
└─────────────────────────────────────────────────────┘
```

## List

The main results container (`ScrollArea` + `egui::Frame`).

- Items rendered via `List::ui()`, iterating `items` without cloning
- Selection tracked by index; wraps at top/bottom
- Keyboard: Arrow Up/Down, Ctrl+N/P to navigate
- Each `List.Item` row: icon (left) + title + subtitle + accessories (right)
- `List.Section`: renders a dimmed section header label above its items
- `List.EmptyView`: when items is empty, renders a centred message + optional icon; extension provides title/description via a sentinel `ExtensionItem` with id `"::empty::"`

## List.Item

Each row in the list:

| Field | Description |
|-------|-------------|
| `title` | Primary label (bold) |
| `subtitle` | Secondary label (dimmed, right of title) |
| `icon` | Left-side icon — see [icons.md](icons.md) |
| `accessories` | Right-side chips: `{ text, icon, tooltip }` array |
| `detail` | Markdown string or `Detail.Metadata` JSON — shown in the detail panel when item is selected |
| `action` | Action string executed on Enter |

## ActionPanel

Opened with Ctrl+K (or Cmd+K on macOS). Rendered as a floating panel over the list.

- Lists all actions for the selected item
- First action is the primary (also triggered by Enter without opening the panel)
- Custom keyboard shortcuts: `shortcut: { modifiers: ["cmd"], key: "c" }` prop registered at render time and active while the list is focused (no panel required)
- Navigation: Arrow Up/Down within panel, Enter to execute, Escape to close

Action types supported:

| Action | Effect |
|--------|--------|
| `Action` with `onAction` | Calls JS `onAction` callback |
| `Action.CopyToClipboard` | Copies `content` to clipboard |
| `Action.OpenInBrowser` | Opens URL via `platform::open()`, hides window |
| `Action.Push` | Pushes a new view onto the navigation stack (see [navigation.md](navigation.md)) |
| `Action.SubmitForm` | Submits the current form |
| `Action.ShowInFinder` | Opens the item's path in the file manager |
| `Action.Trash` | Moves path to trash |

## Detail Panel

Right-side panel, 320 px wide, shown when the selected item has a `detail` value.

Two rendering modes:
- **Markdown**: `detail` is a plain string → rendered as styled text (bold, italic, code blocks, headings)
- **Metadata**: `detail` is a JSON-encoded `Detail.Metadata` tree → rendered as a structured label/value list

`Detail.Metadata` structure:
```
Detail.Metadata.Label { title, text, icon? }
Detail.Metadata.Link  { title, text, target }
Detail.Metadata.TagList { title, tags: [{ text, color? }] }
Detail.Metadata.Separator
```

## Form

Triggered when an extension returns a sentinel `ExtensionItem` with id `"::form::"`. When `form_state` is `Some`, the form takes over the entire central panel — the search bar, mode badge, and nav breadcrumb are all hidden.

Supported field types: `TextField`, `Checkbox`, `Dropdown` (with `Dropdown.Item` children). The Submit button calls `Action.SubmitForm.onSubmit(values)` in the extension. Pressing Escape discards the form and returns to the search view.

## Grid

`Grid` component maps to a `List` with a multi-column tile layout. Each `Grid.Item` renders as a square tile (image + title below). Column count auto-calculated from window width and `itemSize` prop (`small` / `medium` / `large`). Falls back to single-column list layout if tiles don't fit.

## Toast

Overlay notification at the bottom of the window. Rendered via `ToastManager`:

- Styles: `Success` (green), `Failure` (red), `Animated` (spinner)
- Auto-dismisses after 3 s (success/failure) or when replaced (animated)
- Extensions call `showToast({ style, title, message })` from JS; bridges via `ExtensionMessage::ShowToast`
