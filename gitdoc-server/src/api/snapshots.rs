use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::collections::HashMap;
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
