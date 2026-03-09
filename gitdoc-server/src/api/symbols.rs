use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Serialize;
use std::sync::Arc;

use gitdoc_api_types::requests::{SymbolQuery, RefQuery};

use crate::AppState;
use crate::db::SymbolFilters;
use crate::error::GitdocError;

#[derive(Serialize)]
pub struct SymbolWithChildren {
    pub symbol: crate::db::SymbolDetail,
    pub children: Vec<crate::db::SymbolRow>,
}

#[derive(Serialize)]
pub struct SnapshotSymbolResponse {
    pub symbol: crate::db::SymbolDetail,
    pub children: Vec<crate::db::SymbolRow>,
    pub referenced_by_count: i64,
    pub references_count: i64,
}

pub async fn list_symbols(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SymbolQuery>,
) -> Result<Json<Vec<crate::db::SymbolRow>>, GitdocError> {
    let filters = SymbolFilters {
        kind: q.kind,
        visibility: q.visibility,
        file_path: q.file_path,
        include_private: q.include_private.unwrap_or(false),
    };
    let symbols = state.db.list_symbols_for_snapshot(snapshot_id, &filters).await?;
    Ok(Json(symbols))
}

pub async fn get_symbol(
    State(state): State<Arc<AppState>>,
    Path(symbol_id): Path<i64>,
) -> Result<Json<SymbolWithChildren>, GitdocError> {
    let symbol = state.db.get_symbol_by_id(symbol_id).await?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
    let children = state.db.list_symbol_children(symbol_id).await.unwrap_or_default();
    Ok(Json(SymbolWithChildren { symbol, children }))
}

pub async fn get_snapshot_symbol(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<SnapshotSymbolResponse>, GitdocError> {
    let symbol = state.db.get_symbol_by_id(symbol_id).await?
        .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
    let children = state.db.list_symbol_children(symbol_id).await.unwrap_or_default();
    let (referenced_by_count, references_count) = state
        .db
        .count_refs_for_symbol(symbol_id, snapshot_id)
        .await
        .unwrap_or((0, 0));
    Ok(Json(SnapshotSymbolResponse {
        symbol,
        children,
        referenced_by_count,
        references_count,
    }))
}

pub async fn get_symbol_references(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<Vec<crate::db::RefWithSymbol>>, GitdocError> {
    let direction = q.direction.as_deref().unwrap_or("inbound");
    let limit = q.limit.unwrap_or(20);
    let kind_filter = q.kind.as_deref();

    let refs = match direction {
        "outbound" => state.db.get_outbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
        _ => state.db.get_inbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
    };

    Ok(Json(refs))
}

pub async fn get_symbol_implementations(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<crate::db::RefWithSymbol>>, GitdocError> {
    let impls = state.db.get_implementations(symbol_id, snapshot_id).await?;
    Ok(Json(impls))
}
