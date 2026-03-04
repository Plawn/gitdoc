use tree_sitter::{Language, Node, Query};

/// Tree-sitter query for TypeScript top-level symbols.
pub const TS_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @function

(class_declaration
  name: (type_identifier) @name) @class

(interface_declaration
  name: (type_identifier) @name) @interface

(type_alias_declaration
  name: (type_identifier) @name) @type_alias

(enum_declaration
  name: (identifier) @name) @enum_def

(export_statement
  declaration: (_) @decl) @export
"#;

/// Tree-sitter query for JavaScript (no interfaces, type aliases, or enums).
pub const JS_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @function

(class_declaration
  name: (identifier) @name) @class

(export_statement
  declaration: (_) @decl) @export
"#;

pub fn ts_query() -> Query {
    let lang = Language::from(tree_sitter_typescript::LANGUAGE_TYPESCRIPT);
    Query::new(&lang, TS_QUERY).expect("invalid TypeScript tree-sitter query")
}

pub fn tsx_query() -> Query {
    let lang = Language::from(tree_sitter_typescript::LANGUAGE_TSX);
    Query::new(&lang, TS_QUERY).expect("invalid TSX tree-sitter query")
}

pub fn js_query() -> Query {
    let lang = Language::from(tree_sitter_javascript::LANGUAGE);
    Query::new(&lang, JS_QUERY).expect("invalid JavaScript tree-sitter query")
}

pub fn capture_kind(capture_name: &str) -> Option<&'static str> {
    match capture_name {
        "function" => Some("function"),
        "class" => Some("class"),
        "interface" => Some("interface"),
        "type_alias" => Some("type_alias"),
        "enum_def" => Some("enum"),
        "export" => Some("export"),
        _ => None,
    }
}

/// Extract visibility from a TS/JS node.
/// Exported declarations are "public", everything else is "private".
pub fn extract_visibility(node: Node, _source: &[u8]) -> String {
    // If the parent is an export_statement, it's public
    if let Some(parent) = node.parent() {
        if parent.kind() == "export_statement" {
            return "public".to_string();
        }
    }
    // Check for export keyword on the node itself
    if node.kind() == "export_statement" {
        return "public".to_string();
    }
    "private".to_string()
}

/// Extract JSDoc comments (/** ... */) preceding the node.
pub fn extract_doc_comment(node: Node, source: &[u8]) -> Option<String> {
    let mut check = node;
    // If inside an export_statement, check the export's preceding sibling
    if let Some(parent) = node.parent() {
        if parent.kind() == "export_statement" {
            check = parent;
        }
    }

    let sibling = check.prev_sibling()?;
    if sibling.kind() == "comment" {
        let text = std::str::from_utf8(&source[sibling.byte_range()]).unwrap_or("");
        if text.starts_with("/**") {
            let inner = text
                .strip_prefix("/**")
                .and_then(|s| s.strip_suffix("*/"))
                .unwrap_or(text);
            return Some(inner.trim().to_string());
        }
    }
    None
}
