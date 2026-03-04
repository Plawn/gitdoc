use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::AppState;
use crate::db::SymbolFilters;
use crate::error::GitdocError;

pub async fn get_overview(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let snapshot = state.db.get_snapshot(snapshot_id).await?
        .ok_or_else(|| GitdocError::NotFound("snapshot not found".into()))?;

    let docs = state.db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_default();
    let readme = docs.iter().find(|d| {
        let lower = d.file_path.to_lowercase();
        lower == "readme.md" || lower.ends_with("/readme.md")
    });
    let readme_content = if let Some(r) = readme {
        state
            .db
            .get_doc_content(snapshot_id, &r.file_path)
            .await
            .ok()
            .flatten()
            .and_then(|dc| dc.content)
    } else {
        None
    };

    let symbols = state
        .db
        .list_symbols_for_snapshot(
            snapshot_id,
            &SymbolFilters {
                include_private: false,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_default();
    let top_level: Vec<_> = symbols.iter().filter(|s| s.parent_id.is_none()).collect();

    Ok(Json(serde_json::json!({
        "snapshot": snapshot,
        "readme": readme_content,
        "docs": docs,
        "top_level_symbols": top_level,
    })))
}

pub async fn list_docs(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let docs = state.db.list_docs_for_snapshot(snapshot_id).await?;
    Ok(Json(serde_json::json!(docs)))
}

pub async fn get_doc_content(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, path)): Path<(i64, String)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let doc = state.db.get_doc_content(snapshot_id, &path).await?
        .ok_or_else(|| GitdocError::NotFound("doc not found".into()))?;
    Ok(Json(serde_json::json!(doc)))
}

#[derive(Deserialize)]
pub struct SymbolQuery {
    pub kind: Option<String>,
    pub visibility: Option<String>,
    pub file_path: Option<String>,
    pub include_private: Option<bool>,
}

pub async fn list_symbols(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SymbolQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let filters = SymbolFilters {
        kind: q.kind,
        visibility: q.visibility,
        file_path: q.file_path,
        include_private: q.include_private.unwrap_or(false),
    };
    let symbols = state.db.list_symbols_for_snapshot(snapshot_id, &filters).await?;
    Ok(Json(serde_json::json!(symbols)))
}

pub async fn get_symbol(
    State(state): State<Arc<AppState>>,
    Path(symbol_id): Path<i64>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let symbol = state.db.get_symbol_by_id(symbol_id).await?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
    let children = state.db.list_symbol_children(symbol_id).await.unwrap_or_default();
    Ok(Json(serde_json::json!({
        "symbol": symbol,
        "children": children,
    })))
}

pub async fn get_snapshot_symbol(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let symbol = state.db.get_symbol_by_id(symbol_id).await?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
    let children = state.db.list_symbol_children(symbol_id).await.unwrap_or_default();
    let (referenced_by_count, references_count) = state
        .db
        .count_refs_for_symbol(symbol_id, snapshot_id)
        .await
        .unwrap_or((0, 0));
    Ok(Json(serde_json::json!({
        "symbol": symbol,
        "children": children,
        "referenced_by_count": referenced_by_count,
        "references_count": references_count,
    })))
}

#[derive(Deserialize)]
pub struct RefQuery {
    pub direction: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

pub async fn get_symbol_references(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let direction = q.direction.as_deref().unwrap_or("inbound");
    let limit = q.limit.unwrap_or(20);
    let kind_filter = q.kind.as_deref();

    let refs = match direction {
        "outbound" => state.db.get_outbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
        _ => state.db.get_inbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
    };

    Ok(Json(serde_json::json!(refs)))
}

pub async fn get_symbol_implementations(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let impls = state.db.get_implementations(symbol_id, snapshot_id).await?;
    Ok(Json(serde_json::json!(impls)))
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub kind: Option<String>,
    pub include_private: Option<bool>,
}

pub async fn diff_symbols(
    State(state): State<Arc<AppState>>,
    Path((from_id, to_id)): Path<(i64, i64)>,
    Query(q): Query<DiffQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let include_private = q.include_private.unwrap_or(false);
    let filters = SymbolFilters {
        kind: q.kind.clone(),
        include_private,
        ..Default::default()
    };

    let from_symbols = state.db.list_symbols_for_snapshot(from_id, &filters).await?;
    let to_symbols = state.db.list_symbols_for_snapshot(to_id, &filters).await?;

    let from_map: HashMap<&str, &crate::db::SymbolRow> = from_symbols
        .iter()
        .map(|s| (s.qualified_name.as_str(), s))
        .collect();
    let to_map: HashMap<&str, &crate::db::SymbolRow> = to_symbols
        .iter()
        .map(|s| (s.qualified_name.as_str(), s))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for sym in &to_symbols {
        if !from_map.contains_key(sym.qualified_name.as_str()) {
            added.push(serde_json::json!({
                "name": sym.name,
                "qualified_name": sym.qualified_name,
                "kind": sym.kind,
                "visibility": sym.visibility,
                "file_path": sym.file_path,
                "signature": sym.signature,
            }));
        }
    }

    for sym in &from_symbols {
        if !to_map.contains_key(sym.qualified_name.as_str()) {
            removed.push(serde_json::json!({
                "name": sym.name,
                "qualified_name": sym.qualified_name,
                "kind": sym.kind,
                "visibility": sym.visibility,
                "file_path": sym.file_path,
                "signature": sym.signature,
            }));
        }
    }

    for sym in &to_symbols {
        if let Some(from_sym) = from_map.get(sym.qualified_name.as_str()) {
            let mut changes = Vec::new();
            if from_sym.signature != sym.signature {
                changes.push("signature");
            }
            if from_sym.visibility != sym.visibility {
                changes.push("visibility");
            }
            if !changes.is_empty() {
                modified.push(serde_json::json!({
                    "qualified_name": sym.qualified_name,
                    "kind": sym.kind,
                    "changes": changes,
                    "from": {
                        "signature": from_sym.signature,
                        "visibility": from_sym.visibility,
                    },
                    "to": {
                        "signature": sym.signature,
                        "visibility": sym.visibility,
                    },
                }));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "from_snapshot": from_id,
        "to_snapshot": to_id,
        "added": added,
        "removed": removed,
        "modified": modified,
        "summary": {
            "added": added.len(),
            "removed": removed.len(),
            "modified": modified.len(),
        },
    })))
}

pub async fn delete_snapshot(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let existed = state.db.delete_snapshot(snapshot_id).await?;
    if !existed {
        return Err(GitdocError::NotFound("snapshot not found".into()));
    }
    let gc_stats = state.db.gc_orphans().await?;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "gc": gc_stats,
    })))
}

// =============================================================================
// P0 — Public API Surface
// =============================================================================

#[derive(Deserialize)]
pub struct PublicApiQuery {
    pub module_path: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// A single entry in the public API: a type/function/etc with optionally merged impl methods.
#[derive(Serialize)]
struct PublicApiEntry {
    id: i64,
    name: String,
    qualified_name: String,
    kind: String,
    visibility: String,
    file_path: String,
    signature: String,
    doc_comment: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<PublicApiMethod>,
}

#[derive(Serialize)]
struct PublicApiMethod {
    id: i64,
    name: String,
    signature: String,
    doc_comment: Option<String>,
}

pub async fn get_public_api(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<PublicApiQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let limit = q.limit.unwrap_or(2000);
    let offset = q.offset.unwrap_or(0);

    let symbols = state
        .db
        .get_public_api_symbols(snapshot_id, q.module_path.as_deref(), limit, offset)
        .await?;

    // Group symbols: top-level items + impl children merged onto their parent types
    // Strategy: collect all symbols, identify impl blocks, merge their children onto
    // the type with the same name in the same file.

    // First pass: separate impl blocks from other symbols
    let mut type_map: HashMap<(String, String), usize> = HashMap::new(); // (name, file_path) -> index in entries
    let mut entries: Vec<PublicApiEntry> = Vec::new();
    let mut impl_children: Vec<(String, String, PublicApiMethod)> = Vec::new(); // (parent_name, file_path, method)

    for sym in &symbols {
        if sym.kind == "impl" {
            // Skip impl blocks themselves; we'll collect their children
            continue;
        }

        // Check if this symbol's parent is an impl block
        if let Some(parent_id) = sym.parent_id {
            // Find the parent in our symbols list
            if let Some(parent) = symbols.iter().find(|s| s.id == parent_id) {
                if parent.kind == "impl" {
                    // This is a method on an impl block — queue for merging
                    impl_children.push((
                        parent.name.clone(),
                        parent.file_path.clone(),
                        PublicApiMethod {
                            id: sym.id,
                            name: sym.name.clone(),
                            signature: sym.signature.clone(),
                            doc_comment: sym.doc_comment.clone(),
                        },
                    ));
                    continue;
                }
            }
        }

        // Regular top-level symbol
        let key = (sym.name.clone(), sym.file_path.clone());
        let idx = entries.len();
        type_map.insert(key, idx);
        entries.push(PublicApiEntry {
            id: sym.id,
            name: sym.name.clone(),
            qualified_name: sym.qualified_name.clone(),
            kind: sym.kind.clone(),
            visibility: sym.visibility.clone(),
            file_path: sym.file_path.clone(),
            signature: sym.signature.clone(),
            doc_comment: sym.doc_comment.clone(),
            methods: Vec::new(),
        });
    }

    // Second pass: merge impl children onto their parent types
    for (parent_name, file_path, method) in impl_children {
        if let Some(&idx) = type_map.get(&(parent_name.clone(), file_path.clone())) {
            entries[idx].methods.push(method);
        } else {
            // Impl for a type not in our result set (maybe from another file) —
            // create a synthetic entry
            let key = (parent_name.clone(), file_path.clone());
            let idx = entries.len();
            type_map.insert(key, idx);
            entries.push(PublicApiEntry {
                id: 0,
                name: parent_name,
                qualified_name: String::new(),
                kind: "type".into(),
                visibility: "pub".into(),
                file_path,
                signature: String::new(),
                doc_comment: None,
                methods: vec![method],
            });
        }
    }

    // Group by module (derive module from file_path)
    let mut modules: BTreeMap<String, Vec<&PublicApiEntry>> = BTreeMap::new();
    for entry in &entries {
        let module = path_to_module(&entry.file_path);
        modules.entry(module).or_default().push(entry);
    }

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id,
        "module_path": q.module_path,
        "modules": modules,
        "total_items": entries.len(),
    })))
}

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

// =============================================================================
// P1 — Module Tree
// =============================================================================

#[derive(Deserialize)]
pub struct ModuleTreeQuery {
    pub depth: Option<usize>,
    pub include_signatures: Option<bool>,
}

#[derive(Serialize)]
struct ModuleTreeNode {
    name: String,
    path: String,
    doc_comment: Option<String>,
    public_items: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<ModuleTreeNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    symbols: Vec<ModuleTreeSymbol>,
}

#[derive(Serialize, Clone)]
struct ModuleTreeSymbol {
    name: String,
    kind: String,
    signature: String,
}

pub async fn get_module_tree(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<ModuleTreeQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let max_depth = q.depth.unwrap_or(usize::MAX);
    let include_sigs = q.include_signatures.unwrap_or(false);

    let (file_infos, module_syms) = tokio::join!(
        state.db.get_snapshot_file_paths(snapshot_id),
        state.db.get_module_symbols(snapshot_id),
    );
    let file_infos = file_infos?;
    let module_syms = module_syms?;

    // Build module doc comment map: module_path -> doc_comment
    let mut module_docs: HashMap<String, String> = HashMap::new();
    for m in &module_syms {
        if let Some(ref doc) = m.doc_comment {
            let mod_path = path_to_module(&m.file_path);
            module_docs.insert(mod_path, doc.clone());
        }
    }

    // Build a flat map of module_path -> (public_items, file_paths)
    let mut module_info: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
    for fi in &file_infos {
        if fi.file_type == "other" {
            continue;
        }
        let mod_path = path_to_module(&fi.file_path);
        let entry = module_info.entry(mod_path).or_insert((0, Vec::new()));
        entry.0 += fi.public_symbol_count;
        entry.1.push(fi.file_path.clone());
    }

    // Optionally load signatures
    let sig_map: HashMap<String, Vec<ModuleTreeSymbol>> = if include_sigs {
        let all_paths: Vec<String> = file_infos.iter().map(|f| f.file_path.clone()).collect();
        let syms = state.db.get_public_signatures_by_file(snapshot_id, &all_paths).await?;
        let mut m: HashMap<String, Vec<ModuleTreeSymbol>> = HashMap::new();
        for s in syms {
            if s.kind == "impl" {
                continue;
            }
            let mod_path = path_to_module(&s.file_path);
            m.entry(mod_path).or_default().push(ModuleTreeSymbol {
                name: s.name,
                kind: s.kind,
                signature: s.signature,
            });
        }
        m
    } else {
        HashMap::new()
    };

    // Build tree from flat paths
    let root = build_module_tree(&module_info, &module_docs, &sig_map, max_depth);

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id,
        "tree": root,
    })))
}

fn build_module_tree(
    module_info: &BTreeMap<String, (i64, Vec<String>)>,
    module_docs: &HashMap<String, String>,
    sig_map: &HashMap<String, Vec<ModuleTreeSymbol>>,
    max_depth: usize,
) -> Vec<ModuleTreeNode> {
    // Build a tree by splitting module paths on "::"
    let mut root_children: BTreeMap<String, ModuleTreeBuilder> = BTreeMap::new();

    for (path, (count, _)) in module_info {
        let parts: Vec<&str> = if path == "crate" {
            vec!["crate"]
        } else {
            path.split("::").collect()
        };

        if parts.is_empty() {
            continue;
        }

        let mut current = &mut root_children;
        for (i, part) in parts.iter().enumerate() {
            let partial_path = parts[..=i].join("::");
            let node = current.entry(part.to_string()).or_insert_with(|| ModuleTreeBuilder {
                path: partial_path.clone(),
                doc_comment: module_docs.get(&partial_path).cloned(),
                public_items: 0,
                children: BTreeMap::new(),
                symbols: Vec::new(),
            });

            if i == parts.len() - 1 {
                node.public_items += count;
                if let Some(sigs) = sig_map.get(&partial_path) {
                    node.symbols = sigs.iter().map(|s| ModuleTreeSymbol {
                        name: s.name.clone(),
                        kind: s.kind.clone(),
                        signature: s.signature.clone(),
                    }).collect();
                }
            }
            current = &mut node.children;
        }
    }

    fn convert(map: &BTreeMap<String, ModuleTreeBuilder>, depth: usize, max_depth: usize) -> Vec<ModuleTreeNode> {
        map.iter().map(|(name, builder)| {
            ModuleTreeNode {
                name: name.clone(),
                path: builder.path.clone(),
                doc_comment: builder.doc_comment.clone(),
                public_items: builder.public_items,
                children: if depth < max_depth {
                    convert(&builder.children, depth + 1, max_depth)
                } else {
                    Vec::new()
                },
                symbols: builder.symbols.clone(),
            }
        }).collect()
    }

    convert(&root_children, 0, max_depth)
}

struct ModuleTreeBuilder {
    path: String,
    doc_comment: Option<String>,
    public_items: i64,
    children: BTreeMap<String, ModuleTreeBuilder>,
    symbols: Vec<ModuleTreeSymbol>,
}

// =============================================================================
// P2 — Type Context
// =============================================================================

pub async fn get_type_context(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    // Run 5 queries in parallel
    let (symbol_res, children_res, impls_res, inbound_res, outbound_res) = tokio::join!(
        state.db.get_symbol_by_id(symbol_id),
        state.db.list_symbol_children(symbol_id),
        state.db.get_implementations(symbol_id, snapshot_id),
        state.db.get_inbound_refs(symbol_id, snapshot_id, None, 50),
        state.db.get_outbound_refs(symbol_id, snapshot_id, None, 50),
    );

    let symbol = symbol_res?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
    let children = children_res.unwrap_or_default();
    let implementations = impls_res.unwrap_or_default();
    let inbound = inbound_res.unwrap_or_default();
    let outbound = outbound_res.unwrap_or_default();

    // Categorize relationships
    let methods: Vec<_> = children.iter()
        .filter(|c| c.kind == "function")
        .collect();
    let fields: Vec<_> = children.iter()
        .filter(|c| c.kind != "function")
        .collect();

    let traits_implemented: Vec<_> = implementations.iter()
        .filter(|r| r.symbol.kind == "trait" || r.symbol.kind == "interface")
        .collect();
    let implementors: Vec<_> = implementations.iter()
        .filter(|r| r.symbol.kind != "trait" && r.symbol.kind != "interface")
        .collect();

    // Separate inbound by kind
    let callers: Vec<_> = inbound.iter().filter(|r| r.ref_kind == "calls").collect();
    let type_users: Vec<_> = inbound.iter().filter(|r| r.ref_kind == "type_ref").collect();

    // Separate outbound by kind
    let dependencies: Vec<_> = outbound.iter().filter(|r| r.ref_kind == "type_ref").collect();
    let calls: Vec<_> = outbound.iter().filter(|r| r.ref_kind == "calls").collect();

    Ok(Json(serde_json::json!({
        "symbol": symbol,
        "methods": methods,
        "fields": fields,
        "traits_implemented": traits_implemented,
        "implementors": implementors,
        "used_by": {
            "callers": callers,
            "type_users": type_users,
        },
        "depends_on": {
            "types": dependencies,
            "calls": calls,
        },
    })))
}

// =============================================================================
// P3 — Code Examples
// =============================================================================

pub async fn get_examples(
    State(state): State<Arc<AppState>>,
    Path((_snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let symbol = state.db.get_symbol_by_id(symbol_id).await?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;

    let examples = if let Some(ref doc) = symbol.doc_comment {
        crate::indexer::doc_parser::extract_code_examples(doc)
    } else {
        Vec::new()
    };

    Ok(Json(serde_json::json!({
        "symbol_id": symbol_id,
        "symbol_name": symbol.name,
        "examples": examples,
    })))
}

// =============================================================================
// P4 — LLM Summaries
// =============================================================================

#[derive(Deserialize)]
pub struct SummarizeQuery {
    /// Scope: "crate", "module:<path>", or "type:<symbol_id>"
    pub scope: String,
}

/// POST /snapshots/{id}/summarize?scope=crate
/// Trigger LLM summary generation (explicit, cost-controlled).
pub async fn summarize(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SummarizeQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let llm_client = state
        .llm_client
        .as_ref()
        .ok_or_else(|| GitdocError::ServiceUnavailable("no LLM provider configured (set GITDOC_LLM_ENDPOINT)".into()))?;

    let content = crate::llm::generate_and_store_summary(
        llm_client.clone(),
        &state.db,
        snapshot_id,
        &q.scope,
    )
    .await
    .map_err(|e| GitdocError::Internal(e))?;

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id,
        "scope": q.scope,
        "content": content,
    })))
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    /// Scope: "crate", "module:<path>", or "type:<symbol_id>"
    pub scope: Option<String>,
}

/// GET /snapshots/{id}/summary?scope=crate
/// Retrieve a previously generated summary.
pub async fn get_summary(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    if let Some(scope) = &q.scope {
        let summary = state
            .db
            .get_summary(snapshot_id, scope)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("no summary for scope '{scope}'. Call POST /summarize first.")))?;
        Ok(Json(serde_json::json!(summary)))
    } else {
        // List all summaries for this snapshot
        let summaries = state.db.list_summaries(snapshot_id).await?;
        Ok(Json(serde_json::json!(summaries)))
    }
}
