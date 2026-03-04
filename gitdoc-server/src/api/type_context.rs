use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;

pub async fn get_type_context(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
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

    let callers: Vec<_> = inbound.iter().filter(|r| r.ref_kind == "calls").collect();
    let type_users: Vec<_> = inbound.iter().filter(|r| r.ref_kind == "type_ref").collect();

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
