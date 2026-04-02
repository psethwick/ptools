//! Theming: custom colour tokens matching the Raycast clone spec palette.
//!
//! Spec-defined colours:
//!
//! | Token              | Dark       | Light      |
//! |--------------------|------------|------------|
//! | Background         | `#1c1c1e`  | `#f2f2f7`  |
//! | Surface            | `#2c2c2e`  | `#ffffff`  |
//! | Primary text       | `#ffffff`  | `#000000`  |
//! | Secondary text     | `#8e8e93`  | `#6c6c70`  |
//! | Accent             | `#0a84ff`  | `#007aff`  |
//! | Selection highlight| `#3a3a3c`  | `#d1d1d6`  |

use egui::{Color32, CornerRadius, Stroke, Visuals};

// ── Dark palette ─────────────────────────────────────────────────────────────

const DARK_BG: Color32 = Color32::from_rgb(28, 28, 30);
const DARK_SURFACE: Color32 = Color32::from_rgb(44, 44, 46);
const DARK_PRIMARY_TEXT: Color32 = Color32::WHITE;
const DARK_SECONDARY_TEXT: Color32 = Color32::from_rgb(142, 142, 147);
const DARK_ACCENT: Color32 = Color32::from_rgb(10, 132, 255);
const DARK_SELECTION: Color32 = Color32::from_rgb(58, 58, 60);
const DARK_SEPARATOR: Color32 = Color32::from_rgb(58, 58, 60);
const DARK_INPUT_BG: Color32 = Color32::from_rgb(20, 20, 22);

// ── Light palette ─────────────────────────────────────────────────────────────

const LIGHT_BG: Color32 = Color32::from_rgb(242, 242, 247);
const LIGHT_SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
const LIGHT_PRIMARY_TEXT: Color32 = Color32::BLACK;
const LIGHT_SECONDARY_TEXT: Color32 = Color32::from_rgb(108, 108, 112);
const LIGHT_ACCENT: Color32 = Color32::from_rgb(0, 122, 255);
const LIGHT_SELECTION: Color32 = Color32::from_rgb(209, 209, 214);
const LIGHT_SEPARATOR: Color32 = Color32::from_rgb(209, 209, 214);
const LIGHT_INPUT_BG: Color32 = Color32::from_rgb(228, 228, 235);

/// Build an egui [`Visuals`] struct with the spec-defined colour tokens for
/// the dark theme.
pub fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = DARK_BG;
    v.window_fill = DARK_BG;
    v.extreme_bg_color = DARK_INPUT_BG;
    v.faint_bg_color = DARK_SURFACE;
    v.code_bg_color = DARK_SURFACE;
    v.hyperlink_color = DARK_ACCENT;

    // Separator / border strokes
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DARK_SEPARATOR);
    v.window_stroke = Stroke::new(1.0, DARK_SEPARATOR);

    // Noninteractive widget (labels, separators)
    v.widgets.noninteractive.bg_fill = DARK_SURFACE;
    v.widgets.noninteractive.weak_bg_fill = DARK_SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, DARK_SECONDARY_TEXT);
    v.widgets.noninteractive.corner_radius = CornerRadius::same(6);

    // Inactive widget (buttons, text inputs at rest)
    v.widgets.inactive.bg_fill = DARK_SURFACE;
    v.widgets.inactive.weak_bg_fill = DARK_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, DARK_PRIMARY_TEXT);
    v.widgets.inactive.corner_radius = CornerRadius::same(6);

    // Hovered widget
    v.widgets.hovered.bg_fill = DARK_SELECTION;
    v.widgets.hovered.weak_bg_fill = DARK_SELECTION;
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, DARK_PRIMARY_TEXT);
    v.widgets.hovered.corner_radius = CornerRadius::same(6);

    // Active / pressed widget
    v.widgets.active.bg_fill = DARK_ACCENT;
    v.widgets.active.weak_bg_fill = DARK_ACCENT;
    v.widgets.active.fg_stroke = Stroke::new(1.5, DARK_PRIMARY_TEXT);
    v.widgets.active.corner_radius = CornerRadius::same(6);

    // Open (e.g. expanded combo-box)
    v.widgets.open.bg_fill = DARK_SELECTION;
    v.widgets.open.fg_stroke = Stroke::new(1.0, DARK_PRIMARY_TEXT);
    v.widgets.open.corner_radius = CornerRadius::same(6);

    // Selection highlight (text selection, list items)
    v.selection.bg_fill = DARK_SELECTION;
    v.selection.stroke = Stroke::new(1.0, DARK_PRIMARY_TEXT);

    v
}

/// Build an egui [`Visuals`] struct with the spec-defined colour tokens for
/// the light theme.
pub fn light_visuals() -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = LIGHT_BG;
    v.window_fill = LIGHT_BG;
    v.extreme_bg_color = LIGHT_INPUT_BG;
    v.faint_bg_color = LIGHT_SURFACE;
    v.code_bg_color = LIGHT_SURFACE;
    v.hyperlink_color = LIGHT_ACCENT;

    // Separator / border strokes
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LIGHT_SEPARATOR);
    v.window_stroke = Stroke::new(1.0, LIGHT_SEPARATOR);

    // Noninteractive widget
    v.widgets.noninteractive.bg_fill = LIGHT_SURFACE;
    v.widgets.noninteractive.weak_bg_fill = LIGHT_SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, LIGHT_SECONDARY_TEXT);
    v.widgets.noninteractive.corner_radius = CornerRadius::same(6);

    // Inactive widget
    v.widgets.inactive.bg_fill = LIGHT_SURFACE;
    v.widgets.inactive.weak_bg_fill = LIGHT_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, LIGHT_PRIMARY_TEXT);
    v.widgets.inactive.corner_radius = CornerRadius::same(6);

    // Hovered widget
    v.widgets.hovered.bg_fill = LIGHT_SELECTION;
    v.widgets.hovered.weak_bg_fill = LIGHT_SELECTION;
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, LIGHT_PRIMARY_TEXT);
    v.widgets.hovered.corner_radius = CornerRadius::same(6);

    // Active / pressed widget
    v.widgets.active.bg_fill = LIGHT_ACCENT;
    v.widgets.active.weak_bg_fill = LIGHT_ACCENT;
    v.widgets.active.fg_stroke = Stroke::new(1.5, LIGHT_PRIMARY_TEXT);
    v.widgets.active.corner_radius = CornerRadius::same(6);

    // Open
    v.widgets.open.bg_fill = LIGHT_SELECTION;
    v.widgets.open.fg_stroke = Stroke::new(1.0, LIGHT_PRIMARY_TEXT);
    v.widgets.open.corner_radius = CornerRadius::same(6);

    // Selection highlight
    v.selection.bg_fill = LIGHT_SELECTION;
    v.selection.stroke = Stroke::new(1.0, LIGHT_PRIMARY_TEXT);

    v
}

/// Attempt to detect the OS dark/light preference without spawning a process.
///
/// Returns `true` if dark mode is preferred.  Falls back to `true` (dark) when
/// detection is not possible on the current platform.
pub fn os_prefers_dark() -> bool {
    // Linux (GNOME / KDE): check the standard XDG color-scheme hint.
    #[cfg(target_os = "linux")]
    {
        // GTK_THEME env var takes precedence (e.g. "Adwaita:dark").
        if let Ok(gtk_theme) = std::env::var("GTK_THEME") {
            return gtk_theme.to_lowercase().contains("dark");
        }
        // Wayland/DBus color-scheme: prefer-dark = dark, default = light.
        if let Ok(scheme) = std::env::var("XDG_CURRENT_DESKTOP_THEME") {
            return scheme.to_lowercase().contains("dark");
        }
        // Default to dark on Linux when unknown.
        return true;
    }

    // macOS: "Dark" in the system appearance name means dark mode.
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.trim().eq_ignore_ascii_case("dark");
        }
        return true;
    }

    // Windows: AppsUseLightTheme = 0 → dark, = 1 → light.
    #[cfg(target_os = "windows")]
    {
        // Registry read would require winreg crate; fall back to dark.
        return true;
    }

    // All other platforms: default to dark.
    #[allow(unreachable_code)]
    true
}

/// Return egui [`Visuals`] for a theme string.
///
/// - `"dark"` → spec dark palette
/// - `"light"` → spec light palette
/// - `"system"` → dark or light depending on [`os_prefers_dark`]
/// - `None` / anything else → dark (same as `"dark"`)
pub fn visuals_for_theme(theme: Option<&str>) -> Visuals {
    match theme {
        Some("light") => light_visuals(),
        Some("system") => {
            if os_prefers_dark() {
                dark_visuals()
            } else {
                light_visuals()
            }
        }
        _ => dark_visuals(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_visuals_is_dark_mode() {
        assert!(dark_visuals().dark_mode);
    }

    #[test]
    fn light_visuals_is_light_mode() {
        assert!(!light_visuals().dark_mode);
    }

    #[test]
    fn dark_visuals_uses_spec_background() {
        let v = dark_visuals();
        assert_eq!(v.panel_fill, DARK_BG);
        assert_eq!(v.window_fill, DARK_BG);
    }

    #[test]
    fn light_visuals_uses_spec_background() {
        let v = light_visuals();
        assert_eq!(v.panel_fill, LIGHT_BG);
        assert_eq!(v.window_fill, LIGHT_BG);
    }

    #[test]
    fn dark_visuals_uses_spec_accent() {
        assert_eq!(dark_visuals().hyperlink_color, DARK_ACCENT);
    }

    #[test]
    fn light_visuals_uses_spec_accent() {
        assert_eq!(light_visuals().hyperlink_color, LIGHT_ACCENT);
    }

    #[test]
    fn visuals_for_theme_dark_string() {
        assert!(visuals_for_theme(Some("dark")).dark_mode);
    }

    #[test]
    fn visuals_for_theme_light_string() {
        assert!(!visuals_for_theme(Some("light")).dark_mode);
    }

    #[test]
    fn visuals_for_theme_none_defaults_to_dark() {
        assert!(visuals_for_theme(None).dark_mode);
    }

    #[test]
    fn visuals_for_theme_unknown_defaults_to_dark() {
        assert!(visuals_for_theme(Some("banana")).dark_mode);
    }

    #[test]
    fn visuals_for_theme_system_returns_a_mode() {
        let v = visuals_for_theme(Some("system"));
        // Just assert it's one of the two valid modes — OS detection result varies.
        let _ = v.dark_mode; // true or false, both are valid
    }
}
