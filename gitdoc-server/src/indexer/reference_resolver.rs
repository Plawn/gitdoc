use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::db::SymbolForRef;

static RE_RUST_USE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"use\s+crate::([^;]+);").unwrap());
static RE_TS_NAMED_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"import\s+\{([^}]+)\}\s+from\s+['"](\.[^'"]+)['"]"#).unwrap());
static RE_TS_DEFAULT_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"import\s+(\w+)\s+from\s+['"](\.[^'"]+)['"]"#).unwrap());
static RE_IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]+").unwrap());
static RE_IMPL_FOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"impl\s+(\w+)\s+for\s+(\w+)").unwrap());
static RE_EXTENDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"class\s+\w+\s+extends\s+(\w+)").unwrap());
static RE_IMPLEMENTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"implements\s+([\w\s,]+)").unwrap());

#[derive(Debug, Clone)]
pub struct DetectedRef {
    pub from_symbol_id: i64,
    pub to_symbol_id: i64,
    pub kind: String,
}

#[derive(Debug, Clone)]
struct SymbolRef {
    id: i64,
    name: String,
    kind: String,
}

struct ImportEntry {
    local_name: String,
    source_file_id: Option<i64>,
    original_name: Option<String>,
}

struct ReferenceResolver {
    symbols_by_name: HashMap<String, Vec<SymbolRef>>,
    symbols_by_file: HashMap<i64, Vec<SymbolRef>>,
    file_path_to_id: HashMap<String, i64>,
}

// Language keywords to exclude from body scanning
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while", "yield", "str", "bool", "i8", "i16", "i32", "i64",
    "i128", "u8", "u16", "u32", "u64", "u128", "f32", "f64", "usize", "isize", "char", "Ok",
    "Err", "Some", "None", "Result", "Option", "Vec", "String", "Box", "Rc", "Arc",
];

const TS_KEYWORDS: &[&str] = &[
    "abstract", "any", "as", "async", "await", "boolean", "break", "case", "catch", "class",
    "const", "continue", "debugger", "declare", "default", "delete", "do", "else", "enum",
    "export", "extends", "false", "finally", "for", "from", "function", "get", "if", "implements",
    "import", "in", "instanceof", "interface", "is", "keyof", "let", "module", "namespace", "new",
    "null", "number", "of", "package", "private", "protected", "public", "readonly", "require",
    "return", "set", "static", "string", "super", "switch", "symbol", "this", "throw", "true",
    "try", "type", "typeof", "undefined", "unique", "unknown", "var", "void", "while", "with",
    "yield", "never", "object", "bigint",
];

impl ReferenceResolver {
    fn new(
        symbols: &[SymbolForRef],
        file_paths: &HashMap<i64, String>,
    ) -> Self {
        let mut symbols_by_name: HashMap<String, Vec<SymbolRef>> = HashMap::new();
        let mut symbols_by_file: HashMap<i64, Vec<SymbolRef>> = HashMap::new();
        let mut file_path_to_id: HashMap<String, i64> = HashMap::new();

        for (file_id, path) in file_paths {
            file_path_to_id.insert(path.clone(), *file_id);
        }

        for sym in symbols {
            let sref = SymbolRef {
                id: sym.id,
                name: sym.name.clone(),
                kind: sym.kind.clone(),
            };
            symbols_by_name
                .entry(sym.name.clone())
                .or_default()
                .push(sref.clone());
            symbols_by_file
                .entry(sym.file_id)
                .or_default()
                .push(sref);
        }

        Self {
            symbols_by_name,
            symbols_by_file,
            file_path_to_id,
        }
    }

    fn resolve_file_id(&self, base_path: &str, import_path: &str) -> Option<i64> {
        // Compute directory of base file
        let base_dir = base_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        // Normalize relative path
        let resolved = if import_path.starts_with("./") || import_path.starts_with("../") {
            let stripped = import_path.strip_prefix("./").unwrap_or(import_path);
            if base_dir.is_empty() {
                stripped.to_string()
            } else {
                format!("{}/{}", base_dir, stripped)
            }
        } else {
            import_path.to_string()
        };

        // Try various extensions/patterns
        let candidates = vec![
            resolved.clone(),
            format!("{}.rs", resolved),
            format!("{}.ts", resolved),
            format!("{}.tsx", resolved),
            format!("{}.js", resolved),
            format!("{}.jsx", resolved),
            format!("{}/mod.rs", resolved),
            format!("{}/index.ts", resolved),
            format!("{}/index.tsx", resolved),
            format!("{}/index.js", resolved),
        ];

        for candidate in candidates {
            if let Some(&fid) = self.file_path_to_id.get(&candidate) {
                return Some(fid);
            }
        }
        None
    }

    fn extract_rust_imports(
        &self,
        source: &str,
        base_path: &str,
    ) -> Vec<ImportEntry> {
        let mut imports = Vec::new();

        // Match: use crate::path::to::thing;
        for cap in RE_RUST_USE.captures_iter(source) {
            let path_str = cap[1].trim();

            // Handle brace groups: use crate::module::{A, B}
            if let Some(brace_start) = path_str.find('{') {
                let prefix = &path_str[..brace_start];
                let prefix = prefix.trim_end_matches("::");
                if let Some(brace_end) = path_str.find('}') {
                    let items = &path_str[brace_start + 1..brace_end];
                    for item in items.split(',') {
                        let item = item.trim();
                        if item.is_empty() {
                            continue;
                        }
                        // Handle "Name as Alias"
                        let (original, local) = if let Some((orig, alias)) = item.split_once(" as ") {
                            (orig.trim().to_string(), alias.trim().to_string())
                        } else {
                            (item.to_string(), item.to_string())
                        };

                        // Try to resolve the module path to a file
                        let module_path = prefix.replace("::", "/");
                        let file_id = self.resolve_file_id(base_path, &format!("src/{}", module_path));

                        imports.push(ImportEntry {
                            local_name: local,
                            source_file_id: file_id,
                            original_name: Some(original),
                        });
                    }
                }
            } else {
                // Simple import: use crate::module::Thing
                let parts: Vec<&str> = path_str.split("::").collect();
                if let Some(last) = parts.last() {
                    let last = last.trim();
                    // Handle "Name as Alias"
                    let (original, local) = if let Some((orig, alias)) = last.split_once(" as ") {
                        (orig.trim().to_string(), alias.trim().to_string())
                    } else {
                        (last.to_string(), last.to_string())
                    };

                    let module_parts = &parts[..parts.len() - 1];
                    let module_path = module_parts.join("/");
                    let file_id = self.resolve_file_id(base_path, &format!("src/{}", module_path));

                    imports.push(ImportEntry {
                        local_name: local,
                        source_file_id: file_id,
                        original_name: Some(original),
                    });
                }
            }
        }

        imports
    }

    fn extract_ts_imports(
        &self,
        source: &str,
        base_path: &str,
    ) -> Vec<ImportEntry> {
        let mut imports = Vec::new();

        // Named imports: import { A, B } from './module'
        for cap in RE_TS_NAMED_IMPORT.captures_iter(source) {
            let names = &cap[1];
            let module_path = &cap[2];
            let file_id = self.resolve_file_id(base_path, module_path);

            for name in names.split(',') {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                let (original, local) = if let Some((orig, alias)) = name.split_once(" as ") {
                    (orig.trim().to_string(), alias.trim().to_string())
                } else {
                    (name.to_string(), name.to_string())
                };
                imports.push(ImportEntry {
                    local_name: local,
                    source_file_id: file_id,
                    original_name: Some(original),
                });
            }
        }

        // Default imports: import Name from './module'
        for cap in RE_TS_DEFAULT_IMPORT.captures_iter(source) {
            let name = cap[1].to_string();
            let module_path = &cap[2];
            let file_id = self.resolve_file_id(base_path, module_path);
            imports.push(ImportEntry {
                local_name: name,
                source_file_id: file_id,
                original_name: None,
            });
        }

        imports
    }

    fn resolve_identifier(
        &self,
        name: &str,
        imports: &[ImportEntry],
        current_file_id: i64,
    ) -> Option<SymbolRef> {
        // Priority 1: imported symbols
        for imp in imports {
            if imp.local_name == name {
                let target_name = imp.original_name.as_deref().unwrap_or(name);
                // If we resolved the file, look there first
                if let Some(fid) = imp.source_file_id {
                    if let Some(file_syms) = self.symbols_by_file.get(&fid) {
                        if let Some(s) = file_syms.iter().find(|s| s.name == target_name) {
                            return Some(s.clone());
                        }
                    }
                }
                // Fall back to global name lookup
                if let Some(candidates) = self.symbols_by_name.get(target_name) {
                    if let Some(s) = candidates.first() {
                        return Some(s.clone());
                    }
                }
            }
        }

        // Priority 2: same-file symbols
        if let Some(file_syms) = self.symbols_by_file.get(&current_file_id) {
            if let Some(s) = file_syms.iter().find(|s| s.name == name) {
                return Some(s.clone());
            }
        }

        // Priority 3: global by name (only if unambiguous or there's just one)
        if let Some(candidates) = self.symbols_by_name.get(name) {
            if candidates.len() == 1 {
                return Some(candidates[0].clone());
            }
        }

        None
    }

    fn infer_ref_kind(target_kind: &str) -> &'static str {
        match target_kind {
            "function" | "method" => "calls",
            "struct" | "class" | "interface" | "type_alias" | "trait" | "enum" => "type_ref",
            _ => "references",
        }
    }

    fn scan_body(
        &self,
        symbol: &SymbolForRef,
        imports: &[ImportEntry],
        keywords: &HashSet<&str>,
    ) -> Vec<DetectedRef> {
        let mut refs = Vec::new();
        let mut seen_targets: HashSet<i64> = HashSet::new();

        // Extract word tokens from body
        for m in RE_IDENT.find_iter(&symbol.body) {
            let word = m.as_str();
            if keywords.contains(word) {
                continue;
            }
            if let Some(target) = self.resolve_identifier(word, imports, symbol.file_id) {
                // Skip self-references
                if target.id == symbol.id {
                    continue;
                }
                // Dedup within this symbol
                if seen_targets.contains(&target.id) {
                    continue;
                }
                seen_targets.insert(target.id);
                refs.push(DetectedRef {
                    from_symbol_id: symbol.id,
                    to_symbol_id: target.id,
                    kind: Self::infer_ref_kind(&target.kind).to_string(),
                });
            }
        }

        refs
    }

    fn extract_structural_rust(&self, symbol: &SymbolForRef) -> Vec<DetectedRef> {
        let mut refs = Vec::new();

        if symbol.kind != "impl_def" {
            return refs;
        }

        // Parse: impl Trait for Type  or  impl Type
        if let Some(cap) = RE_IMPL_FOR.captures(&symbol.body) {
            let trait_name = &cap[1];
            let _type_name = &cap[2];

            // Link impl → trait as "implements"
            if let Some(candidates) = self.symbols_by_name.get(trait_name) {
                for target in candidates {
                    if target.kind == "trait" {
                        refs.push(DetectedRef {
                            from_symbol_id: symbol.id,
                            to_symbol_id: target.id,
                            kind: "implements".to_string(),
                        });
                    }
                }
            }
        }

        refs
    }

    fn extract_structural_ts(&self, symbol: &SymbolForRef) -> Vec<DetectedRef> {
        let mut refs = Vec::new();

        if symbol.kind != "class" {
            return refs;
        }

        // Parse: class X extends Y
        if let Some(cap) = RE_EXTENDS.captures(&symbol.body) {
            let parent_name = &cap[1];
            if let Some(candidates) = self.symbols_by_name.get(parent_name) {
                if let Some(target) = candidates.first() {
                    refs.push(DetectedRef {
                        from_symbol_id: symbol.id,
                        to_symbol_id: target.id,
                        kind: "extends".to_string(),
                    });
                }
            }
        }

        // Parse: class X implements Y, Z
        if let Some(cap) = RE_IMPLEMENTS.captures(&symbol.body) {
            for iface_name in cap[1].split(',') {
                let iface_name = iface_name.trim();
                if iface_name.is_empty() {
                    continue;
                }
                // Stop at '{' or other non-identifier chars
                let iface_name = iface_name.split_whitespace().next().unwrap_or("");
                if iface_name.is_empty() {
                    continue;
                }
                if let Some(candidates) = self.symbols_by_name.get(iface_name) {
                    if let Some(target) = candidates.first() {
                        refs.push(DetectedRef {
                            from_symbol_id: symbol.id,
                            to_symbol_id: target.id,
                            kind: "implements".to_string(),
                        });
                    }
                }
            }
        }

        refs
    }
}

/// Main entry point: resolve references across all symbols.
///
/// - `symbols`: all symbols in the snapshot (with bodies)
/// - `file_contents`: file_id → raw source content
/// - `file_types`: file_id → file type string ("rust", "typescript", etc.)
/// - `file_paths`: file_id → file path
pub fn resolve_references(
    symbols: &[SymbolForRef],
    file_contents: &HashMap<i64, Vec<u8>>,
    file_types: &HashMap<i64, String>,
    file_paths: &HashMap<i64, String>,
) -> Vec<DetectedRef> {
    let resolver = ReferenceResolver::new(symbols, file_paths);

    let rust_keywords: HashSet<&str> = RUST_KEYWORDS.iter().copied().collect();
    let ts_keywords: HashSet<&str> = TS_KEYWORDS.iter().copied().collect();

    let mut all_refs: Vec<DetectedRef> = Vec::new();

    // Group symbols by file
    let mut symbols_by_file: HashMap<i64, Vec<&SymbolForRef>> = HashMap::new();
    for sym in symbols {
        symbols_by_file.entry(sym.file_id).or_default().push(sym);
    }

    for (&file_id, file_syms) in &symbols_by_file {
        let file_type = match file_types.get(&file_id) {
            Some(ft) => ft.as_str(),
            None => continue,
        };
        let file_path = match file_paths.get(&file_id) {
            Some(fp) => fp.as_str(),
            None => continue,
        };

        let keywords = match file_type {
            "rust" => &rust_keywords,
            "typescript" | "tsx" | "javascript" => &ts_keywords,
            _ => continue,
        };

        // Phase A: Extract imports from file source
        let imports = if let Some(content) = file_contents.get(&file_id) {
            let source = String::from_utf8_lossy(content);
            match file_type {
                "rust" => resolver.extract_rust_imports(&source, file_path),
                "typescript" | "tsx" | "javascript" => {
                    resolver.extract_ts_imports(&source, file_path)
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        for sym in file_syms {
            // Phase B: Body scanning
            all_refs.extend(resolver.scan_body(sym, &imports, keywords));

            // Phase C: Structural relations
            match file_type {
                "rust" => all_refs.extend(resolver.extract_structural_rust(sym)),
                "typescript" | "tsx" | "javascript" => {
                    all_refs.extend(resolver.extract_structural_ts(sym))
                }
                _ => {}
            }
        }
    }

    all_refs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(id: i64, file_id: i64, name: &str, kind: &str, body: &str, file_path: &str) -> SymbolForRef {
        SymbolForRef {
            id,
            file_id,
            name: name.to_string(),
            qualified_name: format!("{}::{}", file_path, name),
            kind: kind.to_string(),
            file_path: file_path.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_rust_simple_import() {
        let symbols = vec![
            make_symbol(1, 10, "foo", "function", "pub fn foo() { bar() }", "src/a.rs"),
            make_symbol(2, 20, "bar", "function", "pub fn bar() {}", "src/b.rs"),
        ];
        let mut file_contents = HashMap::new();
        file_contents.insert(10, b"use crate::b::bar;\n\npub fn foo() { bar() }".to_vec());
        file_contents.insert(20, b"pub fn bar() {}".to_vec());

        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        file_types.insert(20, "rust".to_string());

        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/a.rs".to_string());
        file_paths.insert(20, "src/b.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 1 && r.to_symbol_id == 2 && r.kind == "calls"));
    }

    #[test]
    fn test_rust_brace_import() {
        let symbols = vec![
            make_symbol(1, 10, "main", "function", "pub fn main() { Foo::new(); Bar::new(); }", "src/main.rs"),
            make_symbol(2, 20, "Foo", "struct", "pub struct Foo {}", "src/models.rs"),
            make_symbol(3, 20, "Bar", "struct", "pub struct Bar {}", "src/models.rs"),
        ];
        let mut file_contents = HashMap::new();
        file_contents.insert(10, b"use crate::models::{Foo, Bar};\n\npub fn main() { Foo::new(); Bar::new(); }".to_vec());
        file_contents.insert(20, b"pub struct Foo {}\npub struct Bar {}".to_vec());

        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        file_types.insert(20, "rust".to_string());

        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/main.rs".to_string());
        file_paths.insert(20, "src/models.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 1 && r.to_symbol_id == 2 && r.kind == "type_ref"));
        assert!(refs.iter().any(|r| r.from_symbol_id == 1 && r.to_symbol_id == 3 && r.kind == "type_ref"));
    }

    #[test]
    fn test_ts_named_import() {
        let symbols = vec![
            make_symbol(1, 10, "App", "function", "function App() { return render(data) }", "src/App.tsx"),
            make_symbol(2, 20, "render", "function", "export function render() {}", "src/utils.ts"),
        ];
        let mut file_contents = HashMap::new();
        file_contents.insert(10, b"import { render } from './utils';\n\nfunction App() { return render(data) }".to_vec());
        file_contents.insert(20, b"export function render() {}".to_vec());

        let mut file_types = HashMap::new();
        file_types.insert(10, "tsx".to_string());
        file_types.insert(20, "typescript".to_string());

        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/App.tsx".to_string());
        file_paths.insert(20, "src/utils.ts".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 1 && r.to_symbol_id == 2 && r.kind == "calls"));
    }

    #[test]
    fn test_body_scan_same_file() {
        let symbols = vec![
            make_symbol(1, 10, "main", "function", "fn main() { helper() }", "src/lib.rs"),
            make_symbol(2, 10, "helper", "function", "fn helper() {}", "src/lib.rs"),
        ];
        let file_contents = HashMap::new(); // no raw source needed for same-file
        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/lib.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 1 && r.to_symbol_id == 2 && r.kind == "calls"));
    }

    #[test]
    fn test_no_self_reference() {
        let symbols = vec![
            make_symbol(1, 10, "foo", "function", "fn foo() { foo() }", "src/lib.rs"),
        ];
        let file_contents = HashMap::new();
        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/lib.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_structural_rust_impl() {
        let symbols = vec![
            make_symbol(1, 10, "MyTrait", "trait", "pub trait MyTrait { fn do_it(&self); }", "src/lib.rs"),
            make_symbol(2, 10, "MyStruct", "impl_def", "impl MyTrait for MyStruct { fn do_it(&self) {} }", "src/lib.rs"),
        ];
        let file_contents = HashMap::new();
        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/lib.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 2 && r.to_symbol_id == 1 && r.kind == "implements"));
    }

    #[test]
    fn test_structural_ts_extends() {
        let symbols = vec![
            make_symbol(1, 10, "Base", "class", "class Base {}", "src/base.ts"),
            make_symbol(2, 10, "Child", "class", "class Child extends Base {}", "src/base.ts"),
        ];
        let file_contents = HashMap::new();
        let mut file_types = HashMap::new();
        file_types.insert(10, "typescript".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/base.ts".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 2 && r.to_symbol_id == 1 && r.kind == "extends"));
    }

    #[test]
    fn test_ts_implements() {
        let symbols = vec![
            make_symbol(1, 10, "Serializable", "interface", "interface Serializable { serialize(): string }", "src/types.ts"),
            make_symbol(2, 10, "User", "class", "class User implements Serializable { serialize() { return '' } }", "src/types.ts"),
        ];
        let file_contents = HashMap::new();
        let mut file_types = HashMap::new();
        file_types.insert(10, "typescript".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/types.ts".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        assert!(refs.iter().any(|r| r.from_symbol_id == 2 && r.to_symbol_id == 1 && r.kind == "implements"));
    }

    #[test]
    fn test_keywords_excluded() {
        let symbols = vec![
            make_symbol(1, 10, "process", "function", "fn process() { let x = true; if x { return } }", "src/lib.rs"),
        ];
        let file_contents = HashMap::new();
        let mut file_types = HashMap::new();
        file_types.insert(10, "rust".to_string());
        let mut file_paths = HashMap::new();
        file_paths.insert(10, "src/lib.rs".to_string());

        let refs = resolve_references(&symbols, &file_contents, &file_types, &file_paths);
        // Should not create refs to keywords like "true", "return", "let"
        assert!(refs.is_empty());
    }
}
