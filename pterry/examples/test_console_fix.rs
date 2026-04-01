use egui_raycast_clone2::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
use egui_raycast_clone2::js_extension::JsExtension;
use rquickjs::{Context, Function, Runtime};

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

        println!("Testing console availability with manual console setup...");

        // Test manual console setup
        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;

        context.with(|ctx| -> Result<(), Box<dyn std::error::Error>> {
            // Create console object
            let console_obj = rquickjs::Object::new(ctx.clone())?;

            // Add console.log function
            let log_func = Function::new(
                ctx.clone(),
                |_ctx: rquickjs::Ctx,
                 args: rquickjs::prelude::Rest<rquickjs::Value>|
                 -> Result<(), rquickjs::Error> {
                    let mut output = String::new();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            output.push(' ');
                        }
                        // Convert value to string representation
                        let string_val =
                            arg.clone().into_string().ok_or(rquickjs::Error::Unknown)?;
                        output.push_str(&string_val.to_string().unwrap_or_default());
                    }
                    println!("[JS Console] {output}");
                    Ok(())
                },
            )?;

            console_obj.set("log", log_func)?;

            // Set global console
            let global = ctx.globals();
            global.set("console", console_obj)?;

            // Test the console
            ctx.eval::<(), _>(
                r#"
                console.log("Console is working!");
            "#,
            )?;

            Ok(())
        })?;

        println!("Manual console setup successful!");

        // Now test with the actual extension
        let mut test_extension = JsExtension::new(test_metadata, js_code, false).await?;
        test_extension.initialize().await?;

        println!("Testing search with console.log...");
        let results = test_extension.on_search("test").await?;
        println!("Results: {results:?}");

        Ok(())
    })
}
