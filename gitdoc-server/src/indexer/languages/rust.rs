use tree_sitter::{Language, Node, Query};

/// Tree-sitter query for Rust top-level symbols.
pub const RUST_QUERY: &str = r#"
(function_item
  name: (identifier) @name) @function

(struct_item
  name: (type_identifier) @name) @struct_def

(enum_item
  name: (type_identifier) @name) @enum_def

(trait_item
  name: (type_identifier) @name) @trait_def

(impl_item
  type: (type_identifier) @type_name) @impl_def

(type_item
  name: (type_identifier) @name) @type_alias

(const_item
  name: (identifier) @name) @const_def

(static_item
  name: (identifier) @name) @static_def

(mod_item
  name: (identifier) @name) @module

(macro_definition
  name: (identifier) @name) @macro_def
"#;

pub fn query() -> Query {
    let lang = Language::from(tree_sitter_rust::LANGUAGE);
    Query::new(&lang, RUST_QUERY).expect("invalid Rust tree-sitter query")
}

/// Determine the symbol kind from a capture name.
pub fn capture_kind(capture_name: &str) -> Option<&'static str> {
    match capture_name {
        "function" => Some("function"),
        "struct_def" => Some("struct"),
        "enum_def" => Some("enum"),
        "trait_def" => Some("trait"),
        "impl_def" => Some("impl"),
        "type_alias" => Some("type_alias"),
        "const_def" => Some("constant"),
        "static_def" => Some("static"),
        "module" => Some("module"),
        "macro_def" => Some("macro"),
        _ => None,
    }
}

/// Extract visibility from a Rust node (look for `visibility_modifier` child).
pub fn extract_visibility(node: Node, source: &[u8]) -> String {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "visibility_modifier" {
                let text = &source[child.byte_range()];
                let text = std::str::from_utf8(text).unwrap_or("pub");
                return text.to_string();
            }
        }
    }
    "private".to_string()
}

/// Extract doc comments (/// or /** */) immediately preceding the node.
pub fn extract_doc_comment(node: Node, source: &[u8]) -> Option<String> {
    let mut comments = Vec::new();
    let mut sibling = node.prev_sibling();

    while let Some(sib) = sibling {
        if sib.kind() == "line_comment" {
            let text = std::str::from_utf8(&source[sib.byte_range()]).unwrap_or("");
            if let Some(stripped) = text.strip_prefix("///") {
                comments.push(stripped.trim_start().to_string());
            } else {
                break;
            }
        } else if sib.kind() == "block_comment" {
            let text = std::str::from_utf8(&source[sib.byte_range()]).unwrap_or("");
            if text.starts_with("/**") {
                let inner = text
                    .strip_prefix("/**")
                    .and_then(|s| s.strip_suffix("*/"))
                    .unwrap_or(text);
                comments.push(inner.trim().to_string());
                break;
            } else {
                break;
            }
        } else {
            break;
        }
        sibling = sib.prev_sibling();
    }

    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments.join("\n"))
    }
}
