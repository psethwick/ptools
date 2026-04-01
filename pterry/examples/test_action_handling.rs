use egui_raycast_clone2::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
use egui_raycast_clone2::js_extension::JsExtension;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing JavaScript Action Handling ===\n");

    // Create a test extension with action handling
    let test_metadata = ExtensionMetadata {
        name: "action-test".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test extension for action handling".to_string()),
        author: Some("Test".to_string()),
        language: ExtensionLanguage::JavaScript,
        entry_point: "action-test.js".to_string(),
        permissions: vec![],
        auto_load: true,
        title: None,
        preferences: vec![],
        is_development: false,
    };

    let js_code = r#"
        function onAction(action, itemId) {
            console.log('✅ SUCCESS: Test extension onAction called!');
            console.log('Action:', action);
            console.log('Item ID:', itemId);
            
            if (action === 'test-extension:test-action') {
                console.log('✅ SUCCESS: Test action executed correctly!');
            }
        }

        globalThis.onAction = onAction;
    "#;

    println!("1. Creating test extension...");
    let mut test_extension = JsExtension::new(test_metadata.clone(), js_code, false).await?;
    test_extension.initialize().await?;
    println!(
        "   ✓ Test extension '{}' loaded",
        test_extension.metadata().name
    );

    // Test action functionality
    println!("\n2. Testing action functionality...");
    test_extension
        .on_action("test-extension:test-action", Some("test-item"))
        .await?;
    println!("   ✓ Action executed successfully!");

    // Test action without item ID
    println!("\n3. Testing action without item ID...");
    test_extension
        .on_action("test-extension:simple-action", None)
        .await?;
    println!("   ✓ Action without item ID executed successfully!");

    println!("\n=== All tests passed! Action handling is working correctly ===");

    Ok(())
}
