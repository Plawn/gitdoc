use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::db::SymbolFilters;
use crate::error::GitdocError;

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
