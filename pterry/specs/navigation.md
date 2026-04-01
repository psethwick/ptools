# Navigation Stack (Action.Push / useNavigation)

## Overview

Raycast's navigation model is a push/pop stack of views. An extension starts with one root component. `Action.Push` pushes a new component; `useNavigation().pop()` returns to the previous one.

Each stack level has its own search query, item list, and selected index.

## Rust Side: `NavFrame` and `UiState`

Replace the flat item list in `UiState` with `nav_stack: Vec<NavFrame>`:

```rust
struct NavFrame {
    items: Vec<ExtensionItem>,
    search_query: String,
    selected_index: usize,
    title: Option<String>,
    /// True for the synthetic frame pushed when the user navigates into a mode
    /// from the main list. Popping this frame exits the mode and restores global
    /// search, rather than descending further into a JS view.
    is_main_list_root: bool,
}
```

Helper methods on `UiState`:
- `nav_push(title: Option<String>)` — push a JS view frame (clears search box)
- `nav_push_main_list_root()` — push the sentinel frame used when entering a mode from the main list; saves current items, query, and selection before the mode clears them
- `nav_pop()` — pop top frame, restore saved query + selection; returns `NavPopResult`
- `nav_current_mut()` — borrow the active (last) frame

```rust
enum NavPopResult {
    /// Popped a JS view frame; stay in current mode.
    StayInMode,
    /// Popped the main-list-root sentinel; caller should clear `current_mode`
    /// and restore global search.
    ReturnToMainList,
    /// Stack was already empty; caller should hide the window.
    StackEmpty,
}
```

## Mode Entry and the Nav Stack

The nav stack encodes how the user arrived at the current mode, which determines what Escape does at the mode root.

### Navigated from the main list (`Action::EnterMode`)

When the user selects a result in global search that triggers `enter-mode:<name>`:

1. Call `nav_push_main_list_root()` — saves the current list state as a sentinel frame.
2. Set `current_mode = Some(name)`, clear search, clear list, trigger search.

Nav stack on entry: `[main-list-root]`

Escape at mode root: pops the sentinel → `ReturnToMainList` → clear mode, restore list.

### Launched directly (CLI arg / hotkey `LaunchExtension`)

The window opens already scoped to the extension. No main-list frame is pushed:

1. Set `current_mode = Some(name)`, clear search, clear list, trigger search.

Nav stack on entry: `[]`

Escape at mode root: stack is empty → `StackEmpty` → hide window.

### JS `Action.Push` within a mode

`push-view:` messages call `nav_push(title)` as before, adding a JS view frame on top. These pop normally with `pop-view` or Escape, staying within the mode until the bottom of the stack is reached.

## JS Side

Shim changes:
- `_pushTargets: Map<string, Component>` stores pushed components keyed by UUID
- `_ActionPush` reconciler branch: store component, return `"push-view:<uuid>"`
- `globalThis.onAction` detects `"push-view:"` prefix, swaps `_rootComponent`, calls `_doRender()`

```js
function useNavigation() {
  return {
    push: function(component) {
      var id = _uuid();
      _pushTargets.set(id, component);
      raycast.navigate("push-view:" + id);
    },
    pop: function() { raycast.navigate("pop-view"); },
  };
}
```

## Bridge

New `raycast.navigate(action)` native binding sends `ExtensionMessage::Navigate(String)`. Handled in `App::handle_messages()`:
- `"push-view:<id>"` → call `on_action` on extension (renders new component), then `ui.nav_push(None)`
- `"pop-view"` → `ui.nav_pop()`

## UI

- **Breadcrumb bar**: shown when `nav_stack` contains at least one non-`is_main_list_root` frame; format `Root > Page > …`
- **Escape**: priority order applied on each key press:
  1. If action panel is open → close it
  2. If top nav frame is a JS view (`!is_main_list_root`) → pop it
  3. If search query is non-empty → clear query
  4. Call `nav_pop()`:
     - `ReturnToMainList` → clear mode, restore global search
     - `StackEmpty` → hide window
- **Search box**: cleared on push, restored on pop

## Constraints

- Max stack depth: 10
- No animated transitions (egui is immediate-mode)
