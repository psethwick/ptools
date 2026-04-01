use egui_raycast_clone2::extension_manager::ExtensionManager;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing Real Calculator Extension ===\n");

    let manager = ExtensionManager::new();

    // Load the calculator extension
    let calculator_path = PathBuf::from("extensions/calculator.js");
    println!("Loading calculator extension from: {calculator_path:?}");
    manager
        .load_extension(calculator_path, Some("calculator".to_string()))
        .await?;

    println!("✓ Calculator extension loaded");

    // Test search functionality
    println!("\nTesting search functionality:");

    let results = manager
        .handle_search("calculator", "2 + 2".to_string())
        .await?;
    println!("Search results for '2 + 2': {results:?}");

    let results = manager
        .handle_search("calculator", "10 * 5".to_string())
        .await?;
    println!("Search results for '10 * 5': {results:?}");

    // Test action functionality
    println!("\nTesting action functionality:");
    manager
        .handle_action(
            "calculator",
            "copy-result".to_string(),
            Some("calc-result".to_string()),
        )
        .await?;
    println!("✓ Action executed successfully");

    println!("\n=== Calculator extension test complete ===");

    Ok(())
}
