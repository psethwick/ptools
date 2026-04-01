# Theming

## Theme Options

Three modes selectable in settings:
- **Dark** (default)
- **Light**
- **System** — follows the OS light/dark preference via `winit`'s theme-change event

## Colour Tokens

The egui `Style` is overridden with a palette derived from the active theme. Key tokens:

| Token | Dark | Light |
|-------|------|-------|
| Background | `#1c1c1e` | `#f2f2f7` |
| Surface | `#2c2c2e` | `#ffffff` |
| Primary text | `#ffffff` | `#000000` |
| Secondary text | `#8e8e93` | `#6c6c70` |
| Accent | `#0a84ff` | `#007aff` |
| Selection highlight | `#3a3a3c` | `#d1d1d6` |

## Implementation

- `src/theme.rs` — `Theme` enum + `apply_theme(ctx, theme)` that sets `ctx.style_mut()`
- Theme stored in `Settings.theme: Theme`; applied each frame if changed
- `winit` theme-change event re-applies the system-derived theme when in System mode
- egui `Visuals` fields set: `dark_mode`, panel backgrounds, widget fills, text colours, rounding
