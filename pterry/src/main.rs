use pterry::app::{App, parse_extension_arg, parse_query_arg};

fn main() -> Result<(), eframe::Error> {
    let cli_args: Vec<String> = std::env::args().collect();

    // --help / -h: print usage and exit.
    if cli_args.iter().any(|a| a == "--help" || a == "-h") {
        println!(concat!(
            "Usage: pterry [OPTIONS]\n",
            "\nOptions:\n",
            "  --extension <name>   Open with this extension's mode pre-activated.\n",
            "  --show               Show the window (or trigger via Wayland socket).\n",
            "  --query <text>       Pre-fill the search box with this text on launch.\n",
            "  --help               Print this help.",
        ));
        return Ok(());
    }

    // --show: connect to the Wayland fallback Unix socket and exit.
    #[cfg(target_os = "linux")]
    if cli_args.iter().any(|a| a == "--show") {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        let path = format!("{dir}/launcher.sock");
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(b"show\n");
        } else {
            eprintln!("pterry: could not connect to socket {path}");
        }
        return Ok(());
    }

    let initial_mode = parse_extension_arg(&cli_args);
    let initial_query = parse_query_arg(&cli_args);
    let viewport_builder = egui::ViewportBuilder::default()
        .with_inner_size(egui::vec2(600.0, 400.0))
        .with_min_inner_size(egui::vec2(600.0, 400.0))
        .with_resizable(false)
        .with_drag_and_drop(false)
        .with_title("Pterry")
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_visible(true) // Start visible for testing
        .with_decorations(true) // Keep window decorations for now
        .with_transparent(false); // No transparency for now

    #[cfg(target_os = "macos")]
    {
        use egui_winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use window_vibrancy::{NSVisualEffectMaterial, apply_vibrancy};

        viewport_builder.with_setup_on_create(Box::new(
            |viewport_id, _native_window, _gl_window| {
                if let Some(window_handle) = _native_window.window_handle().ok() {
                    if let RawWindowHandle::AppKit(handle) = window_handle.as_raw() {
                        unsafe {
                            let ns_window = handle.ns_window.as_ptr() as *mut objc::runtime::Object;
                            let _ =
                                apply_vibrancy(ns_window, NSVisualEffectMaterial::AppearanceBased);
                        }
                    }
                }
            },
        ));
    }

    #[cfg(target_os = "windows")]
    {
        use egui_winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use window_vibrancy::apply_blur;

        viewport_builder.with_setup_on_create(Box::new(
            |viewport_id, _native_window, _gl_window| {
                if let Some(window_handle) = _native_window.window_handle().ok() {
                    if let RawWindowHandle::Win32(handle) = window_handle.as_raw() {
                        unsafe {
                            let hwnd = handle.hwnd.as_ptr() as *mut _;
                            let _ = apply_blur(hwnd, Some((18, 18, 18, 125)));
                        }
                    }
                }
            },
        ));
    }

    let options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };

    eframe::run_native(
        "Pterry",
        options,
        Box::new(|cc| {
            let settings = pterry::settings::Settings::load();
            // Apply user-configured DPI scale if set; otherwise use the OS native value.
            if let Some(ppp) = settings.pixels_per_point {
                cc.egui_ctx.set_pixels_per_point(ppp);
            }
            // Apply theme (dark by default; "light" for light mode).
            cc.egui_ctx
                .set_visuals(pterry::settings::visuals_for_theme(
                    settings.theme.as_deref(),
                ));
            Ok(Box::new(App::new(
                cc.egui_ctx.clone(),
                initial_mode.clone(),
                initial_query.clone(),
            )?))
        }),
    )
}
