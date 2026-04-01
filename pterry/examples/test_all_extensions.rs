use egui_raycast_clone2::extension_manager::ExtensionManager;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing All JavaScript Extensions ===\n");

    let manager = ExtensionManager::new();

    // Test extensions to load
    let extensions = vec![
        ("simple_test.js", "simple-test"),
        ("simple_test_pure.js", "simple-test-pure"),
        ("action-test.js", "action-test"),
    ];

    for (filename, name) in extensions {
        let path = PathBuf::from(format!("extensions/{filename}"));
        println!("Testing extension: {name} from {filename}");

        match manager.load_extension(path, Some(name.to_string())).await {
            Ok(_) => {
                println!("✓ {name} extension loaded");

                // Test search
                match manager.handle_search(name, "test".to_string()).await {
                    Ok(results) => {
                        println!("  ✓ Search returned {} results", results.len());
                        for result in &results {
                            println!("    - {}", result.title);
                        }
                    }
                    Err(e) => println!("  ✗ Search failed: {e}"),
                }

                // Test action if it's an action-test extension
                if name == "action-test" {
                    match manager
                        .handle_action(
                            name,
                            "test-extension:test-action".to_string(),
                            Some("test-item".to_string()),
                        )
                        .await
                    {
                        Ok(_) => println!("  ✓ Action executed successfully"),
                        Err(e) => println!("  ✗ Action failed: {e}"),
                    }
                }
            }
            Err(e) => println!("✗ Failed to load {name}: {e}"),
        }

        println!();
    }

    println!("=== All extensions test complete ===");

    Ok(())
}
