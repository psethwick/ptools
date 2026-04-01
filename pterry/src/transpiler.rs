use crate::extension_trait::ExtensionError;

/// Transpile TypeScript to JavaScript using oxc (Rust-native, no external deps).
/// Handles interfaces, type annotations, type assertions, JSX, and all TS syntax.
pub(crate) fn transpile_typescript(
    source: &str,
    file_path: &str,
) -> Result<String, ExtensionError> {
    use oxc::allocator::Allocator;
    use oxc::codegen::Codegen;
    use oxc::parser::Parser;
    use oxc::semantic::SemanticBuilder;
    use oxc::span::SourceType;
    use oxc::transformer::{TransformOptions, Transformer};

    let allocator = Allocator::default();
    let path = std::path::Path::new(file_path);

    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::ts());

    let ret = Parser::new(&allocator, source, source_type).parse();

    for error in &ret.errors {
        eprintln!("TypeScript parse warning in {file_path}: {error}");
    }

    let mut program = ret.program;

    let scoping = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions::default();
    let ret =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);

    for error in &ret.errors {
        eprintln!("TypeScript transform error in {file_path}: {error}");
    }

    Ok(Codegen::new().build(&program).code)
}

/// Returns true if the source uses `@raycast/api` imports, meaning it should
/// be treated as a Raycast-compatible extension requiring the CJS shim.
pub(crate) fn is_raycast_api_extension(source: &str) -> bool {
    source.contains("@raycast/api")
}

/// Transpile a Raycast-style TypeScript/JSX extension to CommonJS JavaScript.
///
/// Steps:
/// 1. Parse as TSX (handles TS types + JSX regardless of file extension)
/// 2. oxc transformer: strip TS types + convert JSX to `jsx(...)` calls using
///    the automatic runtime with import source `@raycast/api`
///    (so `<Foo />` becomes `_jsx(Foo, ...)` imported from `@raycast/api/jsx-runtime`)
/// 3. `convert_esm_to_cjs`: text-based pass that rewrites ESM import/export
///    syntax to CommonJS `require()` / `exports.X` (oxc 0.114 has no working
///    ESM→CJS transform, so we do it ourselves)
///
/// The resulting plain script can be `ctx.eval()`'d after injecting the
/// `require`, `exports`, and `module` globals from the @raycast/api shim.
pub(crate) fn transpile_for_raycast(
    source: &str,
    file_path: &str,
) -> Result<String, ExtensionError> {
    use oxc::allocator::Allocator;
    use oxc::codegen::Codegen;
    use oxc::parser::Parser;
    use oxc::semantic::SemanticBuilder;
    use oxc::span::SourceType;
    use oxc::transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};

    let allocator = Allocator::default();
    let path = std::path::Path::new(file_path);

    // Force TSX source type so JSX is always parsed, regardless of file extension
    let source_type = SourceType::from_path(path)
        .unwrap_or_else(|_| SourceType::tsx())
        .with_jsx(true);

    let ret = Parser::new(&allocator, source, source_type).parse();

    for error in &ret.errors {
        eprintln!("Raycast extension parse warning in {file_path}: {error}");
    }

    let mut program = ret.program;

    let scoping = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions {
        // Automatic JSX runtime pointing to @raycast/api so JSX becomes
        // require("@raycast/api/jsx-runtime").jsx(...) after our ESM→CJS pass.
        jsx: JsxOptions {
            runtime: JsxRuntime::Automatic,
            import_source: Some("@raycast/api".to_string()),
            ..JsxOptions::enable()
        },
        ..Default::default()
    };

    let ret =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);

    for error in &ret.errors {
        eprintln!("Raycast extension transform error in {file_path}: {error}");
    }

    let esm_js = Codegen::new().build(&program).code;
    Ok(convert_esm_to_cjs(&esm_js))
}

/// Convert ESM import/export syntax to CommonJS require()/exports.* calls.
///
/// oxc 0.114 lacks a working ESM→CJS transform for regular import/export, so we
/// do a simple line-by-line text pass.  This handles the patterns emitted by
/// oxc codegen for typical Raycast extensions:
///   import { X, Y } from 'mod'       → const { X, Y } = require("mod");
///   import X from 'mod'              → const X = require("mod").default ?? require("mod");
///   import * as X from 'mod'         → const X = require("mod");
///   export default function Foo()    → function Foo() ... exports.default = Foo;
///   export default <expr>;           → exports.default = <expr>;
///   export function/const/var/class  → strip export, append exports.X = X;
fn convert_esm_to_cjs(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 512);
    out.push_str(
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\n",
    );

    // Deferred assignments for named/default exports of declarations
    let mut deferred: Vec<String> = Vec::new();

    // Accumulator for imports that span multiple lines, e.g.:
    //   import {
    //     foo,
    //     bar
    //   } from 'mod';
    let mut pending_import: Option<String> = None;

    for line in source.lines() {
        let t = line.trim();

        // ── accumulate multi-line import ──────────────────────────────────────
        if let Some(ref mut acc) = pending_import {
            acc.push(' ');
            acc.push_str(t);
            if acc.contains(" from ") {
                let complete = acc.clone();
                pending_import = None;
                if let Some(after) = complete.trim().strip_prefix("import ")
                    && let Some(converted) = convert_import_line(after)
                {
                    out.push_str(&converted);
                    out.push('\n');
                }
            }
            continue;
        }

        // Skip redundant "use strict" / __esModule defineProperty lines emitted by oxc
        if t == "\"use strict\";" || t == "'use strict';" {
            continue;
        }
        if t.contains("__esModule") {
            continue;
        }

        // ── import statement ─────────────────────────────────────────────────
        if let Some(after_import) = t.strip_prefix("import ") {
            // Side-effect import (no bindings): `import "mod"` — always single-line.
            // Single-line import with bindings: always contains " from ".
            // Multi-line import: starts with `import {` but " from " is on a later line.
            if t.contains(" from ")
                || after_import.starts_with('"')
                || after_import.starts_with('\'')
            {
                if let Some(converted) = convert_import_line(after_import) {
                    out.push_str(&converted);
                    out.push('\n');
                }
            } else {
                // Begin accumulating a multi-line import
                pending_import = Some(t.to_string());
            }
            continue;
        }

        // ── export default function / class ──────────────────────────────────
        if let Some(rest) = t.strip_prefix("export default function ") {
            // rest = "Name(...) { ..."  or  "(...) { ..." (anonymous)
            let name = rest.split(['(', ' ']).next().filter(|n| {
                !n.is_empty()
                    && n.chars()
                        .next()
                        .map(|c| c.is_alphabetic() || c == '_')
                        .unwrap_or(false)
            });
            if let Some(n) = name {
                out.push_str("function ");
                out.push_str(rest);
                out.push('\n');
                deferred.push(format!("exports.default = {n};"));
            } else {
                out.push_str("exports.default = function ");
                out.push_str(rest);
                out.push('\n');
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("export default class ") {
            let name = rest.split([' ', '{']).next().unwrap_or("_Class");
            out.push_str("class ");
            out.push_str(rest);
            out.push('\n');
            deferred.push(format!("exports.default = {name};"));
            continue;
        }
        // export default <expression>;
        if let Some(rest) = t.strip_prefix("export default ") {
            out.push_str("exports.default = ");
            out.push_str(rest);
            out.push('\n');
            continue;
        }

        // ── named exports ─────────────────────────────────────────────────────
        macro_rules! strip_export_decl {
            ($kw:literal) => {
                if let Some(rest) = t.strip_prefix(concat!("export ", $kw, " ")) {
                    let name = rest
                        .split(|c: char| c == '(' || c == ' ' || c == '{' || c == '=')
                        .next()
                        .unwrap_or("_");
                    out.push_str($kw);
                    out.push(' ');
                    out.push_str(rest);
                    out.push('\n');
                    deferred.push(format!("exports.{name} = {name};"));
                    continue;
                }
            };
        }
        strip_export_decl!("function");
        strip_export_decl!("class");
        strip_export_decl!("const");
        strip_export_decl!("let");
        strip_export_decl!("var");

        // ── export * from 'mod' ───────────────────────────────────────────────
        if let Some(rest) = t.strip_prefix("export * from ") {
            let mod_name = rest
                .trim()
                .trim_matches(|c| c == '"' || c == '\'' || c == ';');
            out.push_str(&format!(
                "Object.assign(exports, require(\"{mod_name}\"));\n"
            ));
            continue;
        }

        // ── export { X, Y } or export { X } from 'mod' ───────────────────────
        if let Some(rest) = t.strip_prefix("export {") {
            if let Some(brace_end) = rest.find('}') {
                let inner = &rest[..brace_end];
                let after_brace = rest[brace_end + 1..].trim().trim_matches(';');
                // Check if there's a "from 'mod'" clause
                let source_mod = if let Some(from_rest) = after_brace.strip_prefix("from ") {
                    let m = from_rest
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == ';');
                    Some(m.to_string())
                } else {
                    None
                };
                for spec in inner.split(',') {
                    let spec = spec.trim();
                    if spec.is_empty() {
                        continue;
                    }
                    let (orig, exported) = if let Some((o, e)) = spec.split_once(" as ") {
                        (o.trim(), e.trim())
                    } else {
                        (spec, spec)
                    };
                    if let Some(ref mod_name) = source_mod {
                        out.push_str(&format!(
                            "exports.{exported} = require(\"{mod_name}\").{orig};\n"
                        ));
                    } else {
                        out.push_str(&format!("exports.{exported} = {orig};\n"));
                    }
                }
            }
            continue;
        }

        // ── everything else — keep as-is ──────────────────────────────────────
        out.push_str(line);
        out.push('\n');
    }

    for d in &deferred {
        out.push_str(d);
        out.push('\n');
    }

    out
}

/// Convert a single ESM import clause (everything after `import `) to a
/// `require()` call.  Returns `None` if the line isn't a recognised pattern.
fn convert_import_line(after_import: &str) -> Option<String> {
    // Trim trailing semicolons and whitespace
    let s = after_import.trim_end_matches(';').trim();

    // import "module"  (side-effect import)
    if s.starts_with('"') || s.starts_with('\'') {
        let mod_name = s.trim_matches(|c| c == '"' || c == '\'');
        return Some(format!("require(\"{mod_name}\");"));
    }

    // Locate "from 'module'" at the end
    let from_idx = s.rfind(" from ")?;
    let module_raw = s[from_idx + 6..].trim();
    let module_name = module_raw.trim_matches(|c| c == '"' || c == '\'' || c == ';');
    let bindings = s[..from_idx].trim();

    // { named } imports
    if bindings.starts_with('{') && bindings.ends_with('}') {
        let inner = &bindings[1..bindings.len() - 1];
        let dest = inner
            .split(',')
            .map(|n| {
                let n = n.trim();
                // "X as Y" → "X: Y"
                if let Some((orig, alias)) = n.split_once(" as ") {
                    format!("{}: {}", orig.trim(), alias.trim())
                } else {
                    n.to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!("const {{ {dest} }} = require(\"{module_name}\");"));
    }

    // * as Namespace
    if let Some(ns) = bindings.strip_prefix("* as ") {
        return Some(format!("const {ns} = require(\"{module_name}\");"));
    }

    // Mixed: Default, { Named }
    if let Some(comma) = bindings.find(", {") {
        let default_name = bindings[..comma].trim();
        let named_part = bindings[comma + 2..].trim(); // includes { }
        let inner = named_part.trim_matches(|c| c == '{' || c == '}' || c == ' ');
        let dest = inner
            .split(',')
            .map(|n| {
                let n = n.trim();
                if let Some((orig, alias)) = n.split_once(" as ") {
                    format!("{}: {}", orig.trim(), alias.trim())
                } else {
                    n.to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let safe = module_name.replace(|c: char| !c.is_alphanumeric(), "_");
        return Some(format!(
            "const _m_{safe} = require(\"{module_name}\"); \
             const {default_name} = _m_{safe}.default ?? _m_{safe}; \
             const {{ {dest} }} = _m_{safe};"
        ));
    }

    // Default import only
    if !bindings.contains(' ') && !bindings.is_empty() {
        // Simple identifier
        let safe = module_name.replace(|c: char| !c.is_alphanumeric(), "_");
        return Some(format!(
            "const _m_{safe} = require(\"{module_name}\"); \
             const {bindings} = _m_{safe}.default ?? _m_{safe};"
        ));
    }

    // Unrecognised — omit and warn
    eprintln!("[raycast-cjs] unhandled import: import {after_import}");
    None
}

#[cfg(test)]
mod tests {
    use super::{
        convert_esm_to_cjs, is_raycast_api_extension, transpile_for_raycast, transpile_typescript,
    };
    use crate::extension_trait::{Extension, ExtensionLanguage, ExtensionMetadata};
    use crate::js_extension::JsExtension;

    #[test]
    fn print_form_test_transpile() {
        let source = include_str!("../extensions/form-test.tsx");
        let js = transpile_for_raycast(source, "form-test.tsx").unwrap();
        println!("{js}");
    }

    #[tokio::test]
    async fn form_test_on_search_returns_sentinel() {
        let source = include_str!("../extensions/form-test.tsx");
        let js = transpile_for_raycast(source, "form-test.tsx").unwrap();
        let metadata = ExtensionMetadata {
            name: "form-test".to_string(),
            version: "0.0.1".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::TypeScript,
            entry_point: "form-test.tsx".to_string(),
            permissions: vec![],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        let ext = JsExtension::new(metadata, &js, true).await.unwrap();
        // Call on_search several times to simulate real-app usage
        for query in &["", "form", "", "test"] {
            let results = ext.on_search(query).await;
            println!("on_search({query:?}) result: {results:?}");
            let items = results.expect("on_search should not error");
            assert_eq!(items.len(), 1, "query={query:?}");
            assert_eq!(items[0].id.as_deref(), Some("::form::"), "query={query:?}");
        }
    }

    /// Type annotations on parameters and remove types are stripped.
    #[test]
    fn ts_type_annotations_are_stripped() {
        let ts = r#"
function greet(name: string): string {
    return 'Hello, ' + name;
}
"#;
        let js = transpile_typescript(ts, "test.ts").unwrap();
        assert!(
            !js.contains(": string"),
            "type annotations should be removed"
        );
        assert!(js.contains("function greet"), "function body should remain");
        assert!(js.contains("return"), "return statement should remain");
    }

    /// Interface declarations are stripped entirely (not emitted to JS output).
    #[test]
    fn ts_interface_declarations_are_stripped() {
        let ts = r#"
interface Item {
    title: string;
    action: string;
    subtitle?: string;
}
function makeItem(title: string): Item {
    return { title: title, action: 'ok' };
}
"#;
        let js = transpile_typescript(ts, "test.ts").unwrap();
        assert!(
            !js.contains("interface"),
            "interface keyword should be stripped"
        );
        assert!(js.contains("function makeItem"), "function should remain");
    }

    /// `as` type assertions are stripped, leaving only the value expression.
    #[test]
    fn ts_type_assertions_are_stripped() {
        let ts = r#"var raycast = (globalThis as any).raycast;"#;
        let js = transpile_typescript(ts, "test.ts").unwrap();
        assert!(!js.contains(" as any"), "type assertion should be stripped");
        assert!(js.contains("globalThis"), "value expression should remain");
    }

    /// TypeScript `async` functions with return-type annotations transpile to
    /// valid JS and execute correctly inside JsExtension.
    #[tokio::test]
    async fn ts_async_function_transpiles_and_runs() {
        let ts = r#"
async function onSearch(query: string): Promise<void> {
    const items: Array<{ title: string; action: string }> = [
        { title: 'async:' + query, action: 'ok' },
    ];
    (globalThis as any).raycast.updateList(items);
}
"#;
        let js = transpile_typescript(ts, "async-test.ts").unwrap();
        let metadata = ExtensionMetadata {
            name: "ts-async".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::TypeScript,
            entry_point: "async-test.ts".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        let ext = JsExtension::new(metadata, &js, false)
            .await
            .expect("async TypeScript should load without error");

        let results = ext
            .on_search("world")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "async:world");
    }

    /// End-to-end: TypeScript with annotations is transpiled to valid JS and
    /// can be loaded and executed by JsExtension with correct results.
    #[tokio::test]
    async fn transpiled_typescript_runs_correctly_in_js_extension() {
        let ts = r#"
interface SearchItem {
    title: string;
    action: string;
}
function buildResults(query: string): SearchItem[] {
    return [
        { title: 'TS result: ' + query, action: 'open-url:https://example.com' },
    ];
}
function onSearch(query: string) {
    if (!query) return;
    const results: SearchItem[] = buildResults(query);
    (globalThis as any).raycast.updateList(results);
}
"#;
        let js = transpile_typescript(ts, "test.ts").unwrap();
        let metadata = ExtensionMetadata {
            name: "ts-test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            language: ExtensionLanguage::TypeScript,
            entry_point: "test.ts".to_string(),
            permissions: vec![],
            auto_load: true,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        let ext = JsExtension::new(metadata, &js, false)
            .await
            .expect("transpiled TypeScript should load without error");

        let results = ext
            .on_search("hello")
            .await
            .expect("on_search should not error");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "TS result: hello");
    }

    // ── multi-line import tests ───────────────────────────────────────────────

    #[test]
    fn multi_line_named_import_is_converted() {
        let js = "import {\n  foo,\n  bar\n} from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"mod\")"),
            "multi-line import should produce require(); got:\n{result}"
        );
        assert!(
            result.contains("foo"),
            "foo should be in destructured binding"
        );
        assert!(
            result.contains("bar"),
            "bar should be in destructured binding"
        );
    }

    #[test]
    fn multi_line_import_does_not_duplicate_or_lose_body() {
        let js = "import {\n  useState,\n  useEffect\n} from 'react';\nconst x = 1;";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"react\")"),
            "react require missing; got:\n{result}"
        );
        assert!(result.contains("useState"));
        assert!(result.contains("useEffect"));
        // body line should still be present
        assert!(result.contains("const x = 1;"));
    }

    #[test]
    fn single_line_import_still_works_after_fix() {
        let js = "import { foo, bar } from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("require(\"mod\")"));
        assert!(result.contains("foo"));
        assert!(result.contains("bar"));
    }

    #[test]
    fn is_raycast_api_extension_detects_import() {
        assert!(is_raycast_api_extension(
            r#"import { List } from "@raycast/api";"#
        ));
        assert!(!is_raycast_api_extension(
            r#"import { something } from "another-lib";"#
        ));
    }

    // ── import pattern tests ──────────────────────────────────────────────────

    #[test]
    fn namespace_import_star_as() {
        let js = "import * as Ns from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("const Ns = require(\"mod\")"),
            "namespace import should produce const Ns = require; got:\n{result}"
        );
    }

    #[test]
    fn default_import_only() {
        let js = "import Foo from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"mod\")"),
            "default import should call require; got:\n{result}"
        );
        assert!(
            result.contains("Foo"),
            "binding name should appear; got:\n{result}"
        );
    }

    #[test]
    fn default_import_uses_default_coalesce() {
        let js = "import React from 'react';";
        let result = convert_esm_to_cjs(js);
        // Should use .default ?? fallback pattern
        assert!(
            result.contains(".default"),
            "default import should use .default coalesce; got:\n{result}"
        );
        assert!(result.contains("React"));
    }

    #[test]
    fn side_effect_import_double_quote() {
        let js = r#"import "polyfill";"#;
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"polyfill\")"),
            "side-effect double-quote import should call require; got:\n{result}"
        );
    }

    #[test]
    fn side_effect_import_single_quote() {
        let js = "import 'polyfill';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"polyfill\")"),
            "side-effect single-quote import should call require; got:\n{result}"
        );
    }

    #[test]
    fn mixed_default_and_named_import() {
        let js = "import Foo, { bar, baz } from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"mod\")"),
            "mixed import should call require; got:\n{result}"
        );
        assert!(result.contains("Foo"), "default binding should appear");
        assert!(result.contains("bar"), "named binding bar should appear");
        assert!(result.contains("baz"), "named binding baz should appear");
    }

    #[test]
    fn named_import_with_alias() {
        let js = "import { foo as f, bar as b } from 'mod';";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("require(\"mod\")"));
        // Aliases should become destructuring renames
        assert!(
            result.contains("foo: f"),
            "aliased import should use foo: f; got:\n{result}"
        );
        assert!(
            result.contains("bar: b"),
            "aliased import should use bar: b; got:\n{result}"
        );
    }

    // ── export pattern tests ──────────────────────────────────────────────────

    #[test]
    fn export_default_expression() {
        let js = "export default 42;";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("exports.default = 42;"),
            "export default expr should become exports.default; got:\n{result}"
        );
    }

    #[test]
    fn export_default_object_literal() {
        let js = "export default { a: 1 };";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("exports.default ="),
            "export default object should become exports.default; got:\n{result}"
        );
    }

    #[test]
    fn export_const_declaration() {
        let js = "export const answer = 42;";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("const answer = 42;"),
            "const declaration should remain; got:\n{result}"
        );
        assert!(
            result.contains("exports.answer = answer;"),
            "deferred export should be appended; got:\n{result}"
        );
    }

    #[test]
    fn export_let_declaration() {
        let js = "export let counter = 0;";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("let counter = 0;"));
        assert!(result.contains("exports.counter = counter;"));
    }

    #[test]
    fn export_var_declaration() {
        let js = "export var legacy = true;";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("var legacy = true;"));
        assert!(result.contains("exports.legacy = legacy;"));
    }

    #[test]
    fn export_function_declaration() {
        let js = "export function greet() { return 'hi'; }";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("function greet()"),
            "function should remain; got:\n{result}"
        );
        assert!(
            result.contains("exports.greet = greet;"),
            "deferred export should be appended; got:\n{result}"
        );
    }

    #[test]
    fn export_class_declaration() {
        let js = "export class MyWidget {}";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("class MyWidget {}"));
        assert!(result.contains("exports.MyWidget = MyWidget;"));
    }

    #[test]
    fn export_default_named_function() {
        let js = "export default function MyComp() { return null; }";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("function MyComp()"),
            "named function should remain; got:\n{result}"
        );
        assert!(
            result.contains("exports.default = MyComp;"),
            "deferred default export should be appended; got:\n{result}"
        );
        assert!(
            !result.contains("export default"),
            "export keyword should be stripped; got:\n{result}"
        );
    }

    #[test]
    fn export_default_named_class() {
        let js = "export default class MyComp {}";
        let result = convert_esm_to_cjs(js);
        assert!(result.contains("class MyComp {}"));
        assert!(result.contains("exports.default = MyComp;"));
    }

    #[test]
    fn deferred_exports_appear_after_body() {
        // Deferred assignments must come after the declaration, not before
        let js = "export const x = 1;\nconst y = x + 1;";
        let result = convert_esm_to_cjs(js);
        let export_pos = result
            .find("exports.x = x;")
            .expect("deferred export missing");
        let body_pos = result.find("const y = x + 1;").expect("body line missing");
        assert!(
            export_pos > body_pos,
            "deferred exports should appear after the body; got:\n{result}"
        );
    }

    // ── documenting currently unhandled / silently-dropped patterns ───────────

    /// `export { X, Y }` should assign each name to exports.
    #[test]
    fn export_brace_assigns_to_exports() {
        let js = "export { foo, bar };";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("exports.foo = foo;"),
            "export {{ foo }} should produce exports.foo = foo; got:\n{result}"
        );
        assert!(
            result.contains("exports.bar = bar;"),
            "export {{ bar }} should produce exports.bar = bar; got:\n{result}"
        );
    }

    /// `export { X as Y }` should assign X to exports.Y.
    #[test]
    fn export_brace_with_alias_assigns_to_exports() {
        let js = "export { foo as myFoo };";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("exports.myFoo = foo;"),
            "export {{ foo as myFoo }} should produce exports.myFoo = foo; got:\n{result}"
        );
    }

    /// `export * from 'mod'` should re-export all via Object.assign.
    #[test]
    fn export_star_from_produces_object_assign() {
        let js = "export * from 'other-mod';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"other-mod\")"),
            "export * from should call require; got:\n{result}"
        );
        assert!(
            result.contains("Object.assign(exports,"),
            "export * from should use Object.assign; got:\n{result}"
        );
    }

    /// `export { X } from 'mod'` should re-export X from module.
    #[test]
    fn export_brace_from_re_export_produces_require() {
        let js = "export { foo } from 'util';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"util\")"),
            "export {{ X }} from 'mod' should call require; got:\n{result}"
        );
        assert!(
            result.contains("exports.foo"),
            "export {{ X }} from 'mod' should set exports.foo; got:\n{result}"
        );
    }

    /// `export { X as Y } from 'mod'` re-export with rename should work.
    #[test]
    fn export_brace_from_re_export_with_alias() {
        let js = "export { foo as bar } from 'util';";
        let result = convert_esm_to_cjs(js);
        assert!(
            result.contains("require(\"util\")"),
            "export {{ X as Y }} from 'mod' should call require; got:\n{result}"
        );
        assert!(
            result.contains("exports.bar"),
            "export {{ foo as bar }} from should set exports.bar; got:\n{result}"
        );
    }
}
