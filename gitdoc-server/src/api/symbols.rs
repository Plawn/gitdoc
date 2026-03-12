use r2e::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use gitdoc_api_types::requests::{SymbolQuery, RefQuery, BatchSymbolsRequest, SymbolContextQuery};

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

#[derive(Serialize)]
pub struct BatchSymbolsResponse {
    pub symbols: Vec<crate::db::SymbolDetail>,
}

#[derive(Serialize)]
pub struct SymbolContextResponse {
    pub symbol: crate::db::SymbolDetail,
    pub children: Vec<crate::db::SymbolRow>,
    pub referenced_by_count: i64,
    pub references_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<crate::db::RefWithSymbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<crate::db::RefWithSymbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations: Option<Vec<crate::db::RefWithSymbol>>,
}

/// Snapshot-scoped symbol routes
#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct SnapshotSymbolController {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[routes]
impl SnapshotSymbolController {
    #[get("/{snapshot_id}/symbols")]
    async fn list_symbols(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<SymbolQuery>,
    ) -> Result<Json<Vec<crate::db::SymbolRow>>, GitdocError> {
        let filters = SymbolFilters {
            kind: q.kind,
            visibility: q.visibility,
            file_path: q.file_path,
            include_private: q.include_private.unwrap_or(false),
        };
        let symbols = self.db.list_symbols_for_snapshot(snapshot_id, &filters).await?;
        Ok(Json(symbols))
    }

    #[get("/{snapshot_id}/symbols/{symbol_id}")]
    async fn get_snapshot_symbol(
        &self,
        Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
    ) -> Result<Json<SnapshotSymbolResponse>, GitdocError> {
        let symbol = self.db.get_symbol_by_id(symbol_id).await?
            .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
        let children = self.db.list_symbol_children(symbol_id).await.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, error = %e, "failed to list symbol children");
            Vec::new()
        });
        let (referenced_by_count, references_count) = self
            .db
            .count_refs_for_symbol(symbol_id, snapshot_id)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(symbol_id, snapshot_id, error = %e, "failed to count refs for symbol");
                (0, 0)
            });
        Ok(Json(SnapshotSymbolResponse {
            symbol,
            children,
            referenced_by_count,
            references_count,
        }))
    }

    #[get("/{snapshot_id}/symbols/{symbol_id}/references")]
    async fn get_symbol_references(
        &self,
        Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
        Query(q): Query<RefQuery>,
    ) -> Result<Json<Vec<crate::db::RefWithSymbol>>, GitdocError> {
        let direction = q.direction.as_deref().unwrap_or("inbound");
        let limit = q.limit.unwrap_or(20);
        let kind_filter = q.kind.as_deref();

        let refs = match direction {
            "outbound" => self.db.get_outbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
            _ => self.db.get_inbound_refs(symbol_id, snapshot_id, kind_filter, limit).await?,
        };

        Ok(Json(refs))
    }

    #[post("/{snapshot_id}/symbols/batch")]
    async fn batch_get_symbols(
        &self,
        Path(snapshot_id): Path<i64>,
        Json(body): Json<BatchSymbolsRequest>,
    ) -> Result<Json<BatchSymbolsResponse>, GitdocError> {
        if body.ids.len() > 100 {
            return Err(GitdocError::BadRequest("max 100 symbol IDs per batch".into()));
        }
        self.db.get_snapshot(snapshot_id).await?
            .ok_or_else(|| GitdocError::NotFound("snapshot not found".into()))?;

        let symbols = self.db.get_symbols_by_ids(&body.ids).await?;
        Ok(Json(BatchSymbolsResponse { symbols }))
    }

    #[get("/{snapshot_id}/symbols/{symbol_id}/context")]
    async fn get_symbol_context(
        &self,
        Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
        Query(q): Query<SymbolContextQuery>,
    ) -> Result<Json<SymbolContextResponse>, GitdocError> {
        let symbol = self.db.get_symbol_by_id(symbol_id).await?
            .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
        let children = self.db.list_symbol_children(symbol_id).await.unwrap_or_default();
        let (referenced_by_count, references_count) = self.db.count_refs_for_symbol(symbol_id, snapshot_id).await.unwrap_or((0, 0));

        let includes: Vec<&str> = q.include.as_deref().unwrap_or("callers,callees,implementations")
            .split(',')
            .map(|s| s.trim())
            .collect();

        let callers = if includes.contains(&"callers") {
            Some(self.db.get_inbound_refs(symbol_id, snapshot_id, None, 50).await?)
        } else {
            None
        };

        let callees = if includes.contains(&"callees") {
            Some(self.db.get_outbound_refs(symbol_id, snapshot_id, None, 50).await?)
        } else {
            None
        };

        let implementations = if includes.contains(&"implementations") {
            Some(self.db.get_implementations(symbol_id, snapshot_id).await?)
        } else {
            None
        };

        Ok(Json(SymbolContextResponse {
            symbol,
            children,
            referenced_by_count,
            references_count,
            callers,
            callees,
            implementations,
        }))
    }

    #[get("/{snapshot_id}/symbols/{symbol_id}/implementations")]
    async fn get_symbol_implementations(
        &self,
        Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
    ) -> Result<Json<Vec<crate::db::RefWithSymbol>>, GitdocError> {
        let impls = self.db.get_implementations(symbol_id, snapshot_id).await?;
        Ok(Json(impls))
    }
}

/// Standalone symbol route (not snapshot-scoped)
#[derive(Controller)]
#[controller(path = "/symbols", state = AppState)]
pub struct SymbolController {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[routes]
impl SymbolController {
    #[get("/{symbol_id}")]
    async fn get_symbol(
        &self,
        Path(symbol_id): Path<i64>,
    ) -> Result<Json<SymbolWithChildren>, GitdocError> {
        let symbol = self.db.get_symbol_by_id(symbol_id).await?
            .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
        let children = self.db.list_symbol_children(symbol_id).await.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, error = %e, "failed to list symbol children");
            Vec::new()
        });
        Ok(Json(SymbolWithChildren { symbol, children }))
    }
}
