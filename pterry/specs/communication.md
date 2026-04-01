# Communication Bridge

## Message System

Event-driven architecture using `crossbeam-channel` (unbounded):

```
Extension → UI:  ExtensionMessage::SearchResults(query: String, items: Vec<ExtensionItem>)
                 ExtensionMessage::ExtensionLoaded(name: String)
                 ExtensionMessage::ShowToast(style, title, message)
                 ExtensionMessage::Navigate(action: String)   // push-view / pop-view
UI → Extension:  extension.on_search(query)  (broadcast)
                 extension.on_action(action, item_id)  (targeted)
```

## Search Flow

1. User types → `App::trigger_search()` broadcasts to all `auto_load: true` extensions
2. Each extension runs `on_search()` on a Tokio task
3. Results arrive as `SearchResults` messages; stale results (wrong query) are discarded
4. `List::set_items()` updates the displayed list each frame

## Action Flow

1. User presses Enter on a selected item
2. `App::execute_action()` parses the action string into a typed `Action` enum
3. Dispatches: platform commands execute in Rust; extension-owned actions call `on_action()` on the owning extension
