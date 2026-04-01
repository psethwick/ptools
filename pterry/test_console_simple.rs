use egui_raycast_clone2::extension_trait::{Extension, ExtensionMetadata, ExtensionLanguage};
use egui_raycast_clone2::js_extension::JsExtension;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let test_metadata = ExtensionMetadata {
        name: "console-test".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test console availability".to_string()),
        author: Some("Test".to_string()),
        language: ExtensionLanguage::JavaScript,
        entry_point: "test_console.js".to_string(),
        permissions: vec![],
    };

    let js_code = std::fs::read_to_string("test_console.js")?;
    
    println!("Testing console availability...");
    let mut test_extension = JsExtension::new(test_metadata, &js_code, false).await?;
    test_extension.initialize().await?;
    
    println!("Testing search with console.log...");
    let results = test_extension.on_search("test").await?;
    println!("Results: {:?}", results);
    
    Ok(())
}
