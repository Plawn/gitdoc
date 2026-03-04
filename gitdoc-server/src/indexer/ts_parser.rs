use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, QueryCursor};

use super::languages::{rust as rust_lang, typescript as ts_lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
}

#[derive(Debug)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: String,
    pub visibility: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub body: String,
    pub line_start: usize,
    pub line_end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Parse a file and extract symbols using tree-sitter.
pub fn parse_file(source: &[u8], lang: SourceLanguage, _file_path: &str) -> Vec<ExtractedSymbol> {
    let mut parser = Parser::new();

    let ts_language = match lang {
        SourceLanguage::Rust => Language::from(tree_sitter_rust::LANGUAGE),
        SourceLanguage::TypeScript => Language::from(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        SourceLanguage::Tsx => Language::from(tree_sitter_typescript::LANGUAGE_TSX),
        SourceLanguage::JavaScript => Language::from(tree_sitter_javascript::LANGUAGE),
    };

    if parser.set_language(&ts_language).is_err() {
        return Vec::new();
    }

    let query = match lang {
        SourceLanguage::Rust => rust_lang::query(),
        SourceLanguage::TypeScript => ts_lang::ts_query(),
        SourceLanguage::Tsx => ts_lang::tsx_query(),
        SourceLanguage::JavaScript => ts_lang::js_query(),
    };

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let root = tree.root_node();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source);

    let mut symbols = Vec::new();

    while let Some(m) = matches.next() {
        let mut name: Option<String> = None;
        let mut outer_node = None;
        let mut kind_str: Option<&str> = None;

        for cap in m.captures {
            let cap_name = &query.capture_names()[cap.index as usize];

            // The outer capture (function, struct_def, etc.) determines the kind
            if let Some(k) = match lang {
                SourceLanguage::Rust => rust_lang::capture_kind(cap_name),
                _ => ts_lang::capture_kind(cap_name),
            } {
                kind_str = Some(k);
                outer_node = Some(cap.node);
            }

            // The @name capture gives us the symbol name
            if *cap_name == "name" || *cap_name == "type_name" {
                name = Some(
                    std::str::from_utf8(&source[cap.node.byte_range()])
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }

        let (Some(name), Some(node), Some(kind)) = (name, outer_node, kind_str) else {
            continue;
        };

        // Skip export wrappers — we'll capture the inner declaration
        if kind == "export" {
            continue;
        }

        let visibility = match lang {
            SourceLanguage::Rust => rust_lang::extract_visibility(node, source),
            _ => ts_lang::extract_visibility(node, source),
        };

        let doc_comment = match lang {
            SourceLanguage::Rust => rust_lang::extract_doc_comment(node, source),
            _ => ts_lang::extract_doc_comment(node, source),
        };

        let body_text = std::str::from_utf8(&source[node.byte_range()])
            .unwrap_or("")
            .to_string();

        // Signature: text before the body block (heuristic: up to first '{')
        let signature = extract_signature(&body_text);

        symbols.push(ExtractedSymbol {
            name,
            kind: kind.to_string(),
            visibility,
            signature,
            doc_comment,
            body: body_text,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            byte_start: node.byte_range().start,
            byte_end: node.byte_range().end,
        });
    }

    symbols
}

/// Extract a signature from source text (everything before the first `{`).
pub(crate) fn extract_signature(body: &str) -> String {
    if let Some(pos) = body.find('{') {
        body[..pos].trim().to_string()
    } else {
        // For items without a body block (type aliases, consts), use the whole thing
        let s = body.trim();
        if s.len() > 200 {
            s[..200].to_string()
        } else {
            s.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rust parsing ---

    #[test]
    fn rust_function() {
        let src = b"pub fn hello(x: i32) -> String {\n    x.to_string()\n}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "hello");
        assert_eq!(syms[0].kind, "function");
        assert_eq!(syms[0].visibility, "pub");
        assert!(syms[0].signature.contains("pub fn hello"));
    }

    #[test]
    fn rust_struct_and_enum() {
        let src = b"pub struct Foo {\n    pub x: i32,\n}\n\nenum Bar {\n    A,\n    B,\n}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"Bar"));
        let foo = syms.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(foo.kind, "struct");
        assert_eq!(foo.visibility, "pub");
        let bar = syms.iter().find(|s| s.name == "Bar").unwrap();
        assert_eq!(bar.kind, "enum");
        assert_eq!(bar.visibility, "private");
    }

    #[test]
    fn rust_trait_and_impl() {
        let src = b"pub trait Greet {\n    fn greet(&self);\n}\n\nimpl Greet for String {\n    fn greet(&self) {}\n}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        let trait_sym = syms.iter().find(|s| s.kind == "trait").unwrap();
        assert_eq!(trait_sym.name, "Greet");
        let impl_sym = syms.iter().find(|s| s.kind == "impl").unwrap();
        assert_eq!(impl_sym.name, "String");
    }

    #[test]
    fn rust_const_static_module_macro() {
        let src = b"pub const MAX: usize = 100;\nstatic COUNT: i32 = 0;\npub mod utils {}\nmacro_rules! my_macro {\n    () => {};\n}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MAX"), "missing const: {names:?}");
        assert!(names.contains(&"COUNT"), "missing static: {names:?}");
        assert!(names.contains(&"utils"), "missing module: {names:?}");
        assert!(names.contains(&"my_macro"), "missing macro: {names:?}");
    }

    #[test]
    fn rust_type_alias() {
        let src = b"pub type Result<T> = std::result::Result<T, Error>;";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Result");
        assert_eq!(syms[0].kind, "type_alias");
    }

    #[test]
    fn rust_doc_comments() {
        let src = b"/// This is a documented function\n/// with two lines\npub fn documented() {}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        assert_eq!(syms.len(), 1);
        let doc = syms[0].doc_comment.as_deref().unwrap();
        assert!(doc.contains("documented function"), "got: {doc}");
        assert!(doc.contains("two lines"), "got: {doc}");
    }

    #[test]
    fn rust_line_positions() {
        let src = b"fn a() {}\n\nfn b() {}";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        let a = syms.iter().find(|s| s.name == "a").unwrap();
        assert_eq!(a.line_start, 1);
        let b = syms.iter().find(|s| s.name == "b").unwrap();
        assert_eq!(b.line_start, 3);
    }

    // --- TypeScript parsing ---

    #[test]
    fn typescript_function_and_class() {
        let src = b"function greet(name: string): void {\n  console.log(name);\n}\n\nclass MyClass {\n  constructor() {}\n}";
        let syms = parse_file(src, SourceLanguage::TypeScript, "test.ts");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"), "names: {names:?}");
        assert!(names.contains(&"MyClass"), "names: {names:?}");
    }

    #[test]
    fn typescript_interface_and_type() {
        let src = b"interface User {\n  name: string;\n}\n\ntype ID = string | number;";
        let syms = parse_file(src, SourceLanguage::TypeScript, "test.ts");
        let iface = syms.iter().find(|s| s.name == "User").unwrap();
        assert_eq!(iface.kind, "interface");
        let alias = syms.iter().find(|s| s.name == "ID").unwrap();
        assert_eq!(alias.kind, "type_alias");
    }

    #[test]
    fn typescript_enum() {
        let src = b"enum Color {\n  Red,\n  Green,\n  Blue,\n}";
        let syms = parse_file(src, SourceLanguage::TypeScript, "test.ts");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Color");
        assert_eq!(syms[0].kind, "enum");
    }

    // --- JavaScript parsing ---

    #[test]
    fn javascript_function_and_class() {
        let src = b"function hello() {\n  return 42;\n}\n\nclass Widget {\n  render() {}\n}";
        let syms = parse_file(src, SourceLanguage::JavaScript, "test.js");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"), "names: {names:?}");
        assert!(names.contains(&"Widget"), "names: {names:?}");
    }

    #[test]
    fn javascript_no_types() {
        // JS parser shouldn't find interfaces or type aliases
        let src = b"function only() {}";
        let syms = parse_file(src, SourceLanguage::JavaScript, "test.js");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].kind, "function");
    }

    // --- TSX parsing ---

    #[test]
    fn tsx_basic() {
        let src = b"function App(): JSX.Element {\n  return <div />;\n}";
        let syms = parse_file(src, SourceLanguage::Tsx, "test.tsx");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "App");
    }

    // --- Signature extraction ---

    #[test]
    fn signature_before_brace() {
        assert_eq!(extract_signature("pub fn foo() {\n  body\n}"), "pub fn foo()");
    }

    #[test]
    fn signature_no_brace() {
        assert_eq!(extract_signature("type X = i32;"), "type X = i32;");
    }

    // --- Empty / invalid input ---

    #[test]
    fn empty_source() {
        let syms = parse_file(b"", SourceLanguage::Rust, "empty.rs");
        assert!(syms.is_empty());
    }

    #[test]
    fn invalid_syntax_still_extracts_what_it_can() {
        // tree-sitter is error-tolerant, so partial parse should work
        let src = b"fn valid() {}\n\nfn broken( {";
        let syms = parse_file(src, SourceLanguage::Rust, "test.rs");
        // Should at least get the valid function
        assert!(syms.iter().any(|s| s.name == "valid"));
    }
}
