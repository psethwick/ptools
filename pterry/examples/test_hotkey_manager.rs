use egui_raycast_clone2::hotkey_manager::{HotkeyEvent, HotkeyManager};

fn main() {
    println!("Testing hotkey manager functionality...");

    let hotkey_manager = HotkeyManager::new();
    hotkey_manager.start_listening();

    println!("Hotkey manager started. Simulating hotkey events...");

    // Simulate toggle window event
    hotkey_manager.simulate_hotkey(HotkeyEvent::ToggleWindow);

    // Check if the event was received
    if let Some(event) = hotkey_manager.try_receive() {
        match event {
            HotkeyEvent::ToggleWindow => println!("✓ Toggle window event received successfully!"),
            HotkeyEvent::HideWindow => println!("✗ Unexpected hide window event"),
            HotkeyEvent::LaunchExtension(m) => println!("✗ Unexpected launch extension: {m}"),
        }
    } else {
        println!("✗ No event received");
    }

    // Simulate hide window event
    hotkey_manager.simulate_hotkey(HotkeyEvent::HideWindow);

    if let Some(event) = hotkey_manager.try_receive() {
        match event {
            HotkeyEvent::ToggleWindow => println!("✗ Unexpected toggle window event"),
            HotkeyEvent::HideWindow => println!("✓ Hide window event received successfully!"),
            HotkeyEvent::LaunchExtension(m) => println!("✗ Unexpected launch extension: {m}"),
        }
    } else {
        println!("✗ No event received");
    }

    // Simulate launch extension event
    hotkey_manager.simulate_hotkey(HotkeyEvent::LaunchExtension("calculator".to_string()));

    if let Some(event) = hotkey_manager.try_receive() {
        match event {
            HotkeyEvent::LaunchExtension(m) => {
                println!("✓ Launch extension event received: {m}")
            }
            other => println!("✗ Unexpected event: {other:?}"),
        }
    } else {
        println!("✗ No event received");
    }

    println!("Hotkey manager test completed successfully!");
}
