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

        let js_code = r#"
            function onSearch(query) {
                console.log("Test console log");
                var results = [{
                    title: "Test",
                    subtitle: "This is a test",
                    action: "test-action"
                }];
                
                if (typeof globalThis.raycast !== "undefined") {
                    globalThis.raycast.updateList(results);
                }
            }
            
            globalThis.onSearch = onSearch;
        "#;

        println!("Testing console availability...");
        let mut test_extension = JsExtension::new(test_metadata, js_code, false).await?;
        test_extension.initialize().await?;

        println!("Testing search with console.log...");
        let results = test_extension.on_search("test").await?;
        println!("Results: {results:?}");

        Ok(())
    })
}
