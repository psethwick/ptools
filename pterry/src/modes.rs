/// Canonical names for every built-in extension mode.
///
/// Use these constants everywhere a mode name is compared, stored, or
/// constructed so that renaming a mode only requires changing one place.
pub const APP_LAUNCHER: &str = "app-launcher";
pub const CALCULATOR: &str = "calculator";
pub const CLIPBOARD_HISTORY: &str = "clipboard-history";
pub const SETTINGS: &str = "settings";
pub const SETTINGS_SET_GITHUB_TOKEN: &str = "settings-set-github-token";
pub const SETTINGS_EXT_PREFS: &str = "settings-ext-prefs";
pub const STORE: &str = "store";
pub const STORE_NATIVE: &str = "store-native";
pub const STORE_RAYCAST: &str = "store-raycast";
pub const WINDOW_SWITCHER: &str = "window-switcher";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_names_are_kebab_case_and_non_empty() {
        for name in [
            APP_LAUNCHER,
            CALCULATOR,
            CLIPBOARD_HISTORY,
            SETTINGS,
            SETTINGS_SET_GITHUB_TOKEN,
            SETTINGS_EXT_PREFS,
            STORE,
            STORE_NATIVE,
            STORE_RAYCAST,
            WINDOW_SWITCHER,
        ] {
            assert!(!name.is_empty(), "mode name must not be empty");
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "mode name '{}' must be kebab-case (lowercase letters and hyphens only)",
                name
            );
        }
    }
}
