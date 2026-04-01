//! Platform-specific helpers for opening files, URLs, and launching applications.

use std::process::Command;

/// Run a command and print any errors to stderr.
fn run(cmd: &str, args: &[&str]) {
    match Command::new(cmd).args(args).output() {
        Ok(result) if result.status.success() => {}
        Ok(result) => eprintln!("{}", String::from_utf8_lossy(&result.stderr)),
        Err(e) => eprintln!("{e}"),
    }
}

/// Launch an application by path or shell command string.
pub fn launch_app(app_path: &str) {
    #[cfg(target_os = "macos")]
    run("open", &[app_path]);

    #[cfg(target_os = "linux")]
    if let Err(e) = Command::new("sh").args(["-c", app_path]).spawn() {
        eprintln!("Failed to launch application: {e}");
    }

    #[cfg(target_os = "windows")]
    run("cmd", &["/C", "start", "", app_path]);
}

/// Reveal a file in the system file manager (Finder on macOS, file manager on Linux/Windows).
pub fn show_in_finder(path: &str) {
    #[cfg(target_os = "macos")]
    run("open", &["-R", path]);

    #[cfg(target_os = "linux")]
    {
        use std::path::Path;
        let parent = Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(path);
        run("xdg-open", &[parent]);
    }

    #[cfg(target_os = "windows")]
    {
        let selector = format!("/select,{path}");
        run("explorer", &[&selector]);
    }
}

/// Move a file to the system trash.
pub fn trash_file(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Finder\" to delete POSIX file \"{}\"",
            path.replace('"', "\\\"")
        );
        run("osascript", &["-e", &script]);
    }

    #[cfg(target_os = "linux")]
    run("gio", &["trash", path]);

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; \
             [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{}','OnlyErrorDialogs','SendToRecycleBin')",
            path.replace('\'', "\\'")
        );
        run("powershell", &["-Command", &script]);
    }
}

/// Return the currently selected text from the OS.
///
/// - Linux/X11: reads the PRIMARY selection via `xclip -selection primary -o`.
///   Falls back to `xsel --primary --output` if xclip is absent.
/// - macOS / Windows: returns an empty string (platform binding not implemented).
///
/// The function never panics; errors (missing tools, no display) silently return "".
pub fn get_selected_text() -> String {
    #[cfg(target_os = "linux")]
    {
        for (cmd, args) in [
            ("xclip", ["-selection", "primary", "-o"].as_slice()),
            ("xsel", &["--primary", "--output"]),
        ] {
            if let Ok(out) = Command::new(cmd).args(args).output()
                && out.status.success()
                && let Ok(text) = String::from_utf8(out.stdout)
            {
                return text;
            }
        }
    }

    String::new()
}

/// Split a focus-window payload (`"<backend>:<id>"`) into its two parts.
/// Returns `None` when no colon separator is present.
fn parse_focus_payload(payload: &str) -> Option<(&str, &str)> {
    payload.split_once(':')
}

/// Raise and focus a window identified by `payload` = "<backend>:<id>".
pub fn focus_window(payload: &str) {
    let Some((backend, id)) = parse_focus_payload(payload) else {
        eprintln!("[window-switcher] invalid focus payload: {payload}");
        return;
    };

    match backend {
        #[cfg(target_os = "linux")]
        "x11" => {
            crate::core_extensions::window_switcher_extension::focus_x11_window(id);
        }
        other => {
            eprintln!("[window-switcher] unknown backend: {other}");
        }
    }
}

/// Open a URL or file path with the system default handler.
pub fn open(target: &str) {
    #[cfg(target_os = "macos")]
    run("open", &[target]);

    #[cfg(target_os = "linux")]
    run("xdg-open", &[target]);

    #[cfg(target_os = "windows")]
    run("cmd", &["/C", "start", "", target]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke-test that the functions exist and can be called; we cannot easily
    // assert side effects in a unit test, but at least verify compilation and
    // that the functions don't panic on a trivially safe no-op.
    #[test]
    fn launch_app_does_not_panic_on_empty() {
        // "true" is a shell built-in that exits 0 on both macOS and Linux.
        // On Windows the empty string is a no-op via `start ""`.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        launch_app("true");

        #[cfg(target_os = "windows")]
        launch_app("cmd /C exit 0");
    }

    // ── parse_focus_payload ───────────────────────────────────────────────────

    #[test]
    fn parse_focus_payload_x11() {
        assert_eq!(
            parse_focus_payload("x11:41943044"),
            Some(("x11", "41943044"))
        );
    }

    #[test]
    fn parse_focus_payload_no_colon_returns_none() {
        assert_eq!(parse_focus_payload("x11"), None);
    }

    #[test]
    fn parse_focus_payload_extra_colons_preserved_in_id() {
        // The id part may contain colons (e.g. compound identifiers); only the
        // first colon is the separator.
        assert_eq!(
            parse_focus_payload("x11:1:2:3"),
            Some(("x11", "1:2:3"))
        );
    }
}
