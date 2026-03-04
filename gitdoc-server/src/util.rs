/// Convert a file path like "src/runtime/builder.rs" to a module path like "runtime::builder"
pub fn path_to_module(file_path: &str) -> String {
    let p = file_path
        .strip_prefix("src/")
        .unwrap_or(file_path);
    let p = p
        .strip_suffix("/mod.rs")
        .or_else(|| p.strip_suffix("/lib.rs"))
        .or_else(|| p.strip_suffix(".rs"))
        .unwrap_or(p);
    if p == "lib" || p == "main" {
        return "crate".to_string();
    }
    p.replace('/', "::")
}
