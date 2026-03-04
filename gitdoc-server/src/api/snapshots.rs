use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::db::SymbolFilters;

pub async fn get_overview(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> impl IntoResponse {
    let snapshot = match state.db.get_snapshot(snapshot_id) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "snapshot not found" })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let docs = state.db.list_docs_for_snapshot(snapshot_id).unwrap_or_default();
    let readme = docs.iter().find(|d| {
        let lower = d.file_path.to_lowercase();
        lower == "readme.md" || lower.ends_with("/readme.md")
    });
    let readme_content = readme.and_then(|r| {
        state
            .db
            .get_doc_content(snapshot_id, &r.file_path)
            .ok()
            .flatten()
            .and_then(|dc| dc.content)
    });

    let symbols = state
        .db
        .list_symbols_for_snapshot(
            snapshot_id,
            &SymbolFilters {
                include_private: false,
                ..Default::default()
            },
        )
        .unwrap_or_default();
    let top_level: Vec<_> = symbols.iter().filter(|s| s.parent_id.is_none()).collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "snapshot": snapshot,
            "readme": readme_content,
            "docs": docs,
            "top_level_symbols": top_level,
        })),
    )
}

pub async fn list_docs(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> impl IntoResponse {
    match state.db.list_docs_for_snapshot(snapshot_id) {
        Ok(docs) => (StatusCode::OK, Json(serde_json::json!(docs))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_doc_content(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, path)): Path<(i64, String)>,
) -> impl IntoResponse {
    match state.db.get_doc_content(snapshot_id, &path) {
        Ok(Some(doc)) => (StatusCode::OK, Json(serde_json::json!(doc))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "doc not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
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
) -> impl IntoResponse {
    let filters = SymbolFilters {
        kind: q.kind,
        visibility: q.visibility,
        file_path: q.file_path,
        include_private: q.include_private.unwrap_or(false),
    };
    match state.db.list_symbols_for_snapshot(snapshot_id, &filters) {
        Ok(symbols) => (StatusCode::OK, Json(serde_json::json!(symbols))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_symbol(
    State(state): State<Arc<AppState>>,
    Path(symbol_id): Path<i64>,
) -> impl IntoResponse {
    match state.db.get_symbol_by_id(symbol_id) {
        Ok(Some(symbol)) => {
            let children = state.db.list_symbol_children(symbol_id).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "symbol": symbol,
                    "children": children,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "symbol not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_snapshot_symbol(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    match state.db.get_symbol_by_id(symbol_id) {
        Ok(Some(symbol)) => {
            let children = state.db.list_symbol_children(symbol_id).unwrap_or_default();
            let (referenced_by_count, references_count) = state
                .db
                .count_refs_for_symbol(symbol_id, snapshot_id)
                .unwrap_or((0, 0));
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "symbol": symbol,
                    "children": children,
                    "referenced_by_count": referenced_by_count,
                    "references_count": references_count,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "symbol not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
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
) -> impl IntoResponse {
    let direction = q.direction.as_deref().unwrap_or("inbound");
    let limit = q.limit.unwrap_or(20);
    let kind_filter = q.kind.as_deref();

    let result = match direction {
        "outbound" => state.db.get_outbound_refs(symbol_id, snapshot_id, kind_filter, limit),
        _ => state.db.get_inbound_refs(symbol_id, snapshot_id, kind_filter, limit),
    };

    match result {
        Ok(refs) => (StatusCode::OK, Json(serde_json::json!(refs))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_symbol_implementations(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    match state.db.get_implementations(symbol_id, snapshot_id) {
        Ok(impls) => (StatusCode::OK, Json(serde_json::json!(impls))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}
