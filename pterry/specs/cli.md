# CLI Interface

The binary accepts flags that are processed before the egui window opens:

```
raycast-clone [OPTIONS]

Options:
  --extension <name>   Open the window with this extension's mode pre-activated.
                       <name> matches extension metadata name or "name:command" for
                       multi-command extensions. Exits with error if not found.
  --show               Show the window (also connects to the Wayland Unix socket,
                       useful for scripting). No-op if already visible.
  --query <text>       Pre-fill the search box with this text on launch.
  --help               Print this help.
```

`--extension` and `--query` are passed into `App::new()` as `LaunchArgs` and applied after extension loading completes.
