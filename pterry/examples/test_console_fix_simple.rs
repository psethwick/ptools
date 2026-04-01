use egui_raycast_clone2::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
use egui_raycast_clone2::js_extension::JsExtension;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let test_metadata = ExtensionMetadata {
            name: "console-test".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Test console availability".to_string()),
            author: Some("Test".to_string()),
            language: ExtensionLanguage::JavaScript,
            entry_point: "test_console.js".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        // Test JavaScript without console.log first
        let js_code_no_console = r#"
            function onSearch(query) {
                var results = [{
                    title: "Test without console",
                    subtitle: "This is a test without console.log",
                    action: "test-action"
                }];
                
                if (typeof globalThis.raycast !== "undefined") {
                    globalThis.raycast.updateList(results);
                }
            }
            
            globalThis.onSearch = onSearch;
        "#;

        println!("Testing without console.log...");
        let mut test_extension =
            JsExtension::new(test_metadata.clone(), js_code_no_console, false).await?;
        test_extension.initialize().await?;

        let results = test_extension.on_search("test").await?;
        println!("Results without console: {results:?}");

        // Now test with console.log to confirm the issue
        let js_code_with_console = r#"
            function onSearch(query) {
                console.log("Test console log");
                var results = [{
                    title: "Test with console",
                    subtitle: "This is a test with console.log",
                    action: "test-action"
                }];
                
                if (typeof globalThis.raycast !== "undefined") {
                    globalThis.raycast.updateList(results);
                }
            }
            
            globalThis.onSearch = onSearch;
        "#;

        println!("\nTesting with console.log...");
        let mut test_extension_with_console =
            JsExtension::new(test_metadata, js_code_with_console, false).await?;
        test_extension_with_console.initialize().await?;

        match test_extension_with_console.on_search("test").await {
            Ok(results) => println!("Results with console: {results:?}"),
            Err(e) => println!("Error with console.log: {e}"),
        }

        Ok(())
    })
}
