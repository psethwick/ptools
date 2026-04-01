use egui_raycast_clone2::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
use egui_raycast_clone2::js_extension::JsExtension;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Minimal Action Test ===\n");

    let test_metadata = ExtensionMetadata {
        name: "minimal-test".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Minimal test extension".to_string()),
        author: Some("Test".to_string()),
        language: ExtensionLanguage::JavaScript,
        entry_point: "minimal-test.js".to_string(),
        permissions: vec![],
        auto_load: true,
        title: None,
        preferences: vec![],
        is_development: false,
    };

    // Test with the simplest possible JavaScript
    let js_code = r#"
        function onAction(action, itemId) {
            console.log('Action received:', action);
            console.log('Item ID:', itemId);
        }

        function onSearch(query) {
            console.log('Search received:', query);
        }

        globalThis.onAction = onAction;
        globalThis.onSearch = onSearch;
    "#;

    println!("Creating minimal test extension...");
    let mut test_extension = JsExtension::new(test_metadata.clone(), js_code, false).await?;
    test_extension.initialize().await?;
    println!("✓ Extension loaded successfully");

    println!("\nTesting action with item ID...");
    test_extension
        .on_action("test:action", Some("test-item"))
        .await?;
    println!("✓ Action with item ID executed successfully!");

    println!("\nTesting action without item ID...");
    test_extension.on_action("simple:action", None).await?;
    println!("✓ Action without item ID executed successfully!");

    // Test search function to verify basic QuickJS functionality works
    println!("\nTesting search function...");
    let results = test_extension.on_search("test").await?;
    println!("✓ Search returned {} results", results.len());

    println!("\n=== Test completed successfully! ===");

    Ok(())
}
