use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::{DocSearchQuery, SymbolSearchQuery, SemanticSearchQuery};

use crate::AppState;
use crate::db;
use crate::embeddings;
use crate::error::GitdocError;

#[derive(serde::Serialize)]
pub struct SemanticHit {
    source_type: String,
    source_id: i64,
    score: f64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc: Option<DocMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<SymbolMeta>,
}

#[derive(serde::Serialize)]
pub struct DocMeta {
    file_path: String,
    title: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SymbolMeta {
    name: String,
    qualified_name: String,
    kind: String,
    signature: String,
    file_path: String,
    line_start: i64,
}

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct SearchController {
    #[inject]
    db: Arc<db::Database>,
    #[inject]
    search: Arc<crate::search::SearchIndex>,
    #[inject]
    embedder: Option<Arc<dyn embeddings::EmbeddingProvider>>,
}

#[routes]
impl SearchController {
    #[get("/{snapshot_id}/search/docs")]
    async fn search_docs(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<DocSearchQuery>,
    ) -> Result<Json<Vec<crate::search::DocSearchResult>>, GitdocError> {
        if q.q.is_empty() {
            return Err(GitdocError::BadRequest("q must be non-empty".into()));
        }

        let file_ids = self.db.get_file_ids_for_snapshot(snapshot_id).await?;
        let search = self.search.clone();
        let limit = q.limit.unwrap_or(10);
        let query_str = q.q;

        let result = tokio::task::spawn_blocking(move || {
            search.search_docs(&query_str, &file_ids, limit)
        })
        .await??;

        Ok(Json(result))
    }

    #[get("/{snapshot_id}/search/symbols")]
    async fn search_symbols(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<SymbolSearchQuery>,
    ) -> Result<Json<Vec<crate::search::SymbolSearchResult>>, GitdocError> {
        if q.q.is_empty() {
            return Err(GitdocError::BadRequest("q must be non-empty".into()));
        }

        let file_ids = self.db.get_file_ids_for_snapshot(snapshot_id).await?;
        let search = self.search.clone();
        let limit = q.limit.unwrap_or(10);
        let query_str = q.q;
        let kind = q.kind;
        let visibility = q.visibility;

        let result = tokio::task::spawn_blocking(move || {
            search.search_symbols(
                &query_str,
                &file_ids,
                kind.as_deref(),
                visibility.as_deref(),
                limit,
            )
        })
        .await??;

        Ok(Json(result))
    }

    #[get("/{snapshot_id}/search/semantic")]
    async fn search_semantic(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<SemanticSearchQuery>,
    ) -> Result<Json<Vec<SemanticHit>>, GitdocError> {
        if q.q.is_empty() {
            return Err(GitdocError::BadRequest("q must be non-empty".into()));
        }

        let embedder = self.embedder.as_ref()
            .ok_or_else(|| GitdocError::ServiceUnavailable("no embedding provider configured".into()))?;
        let embedder = embedder.clone();

        let db = self.db.clone();
        let limit = q.limit.unwrap_or(10);
        let scope = q.scope.unwrap_or_else(|| "all".into());
        let query_str = q.q;

        let query_vec = embedder.embed_query(&query_str).await
            .map_err(|e| GitdocError::Internal(e))?;

        let file_ids = db.get_file_ids_for_snapshot(snapshot_id).await?;

        let query_pgvec = embeddings::to_pgvector(&query_vec);
        let search_results = db
            .search_embeddings_by_vector(&query_pgvec, &file_ids, &scope, limit as i64)
            .await?;

        let docs = db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_else(|e| {
            tracing::warn!(snapshot_id, error = %e, "failed to list docs for snapshot");
            Vec::new()
        });
        let mut hits: Vec<SemanticHit> = Vec::with_capacity(search_results.len());
        for r in &search_results {
            let mut hit = SemanticHit {
                source_type: r.source_type.clone(),
                source_id: r.source_id,
                score: r.score,
                text: r.text.clone(),
                doc: None,
                symbol: None,
            };

            match r.source_type.as_str() {
                "doc_chunk" => {
                    if let Some(doc) = docs.iter().find(|d| d.id == r.source_id) {
                        hit.doc = Some(DocMeta {
                            file_path: doc.file_path.clone(),
                            title: doc.title.clone(),
                        });
                    }
                }
                "symbol" => {
                    if let Ok(Some(sym)) = db.get_symbol_by_id(r.source_id).await {
                        hit.symbol = Some(SymbolMeta {
                            name: sym.name,
                            qualified_name: sym.qualified_name,
                            kind: sym.kind,
                            signature: sym.signature,
                            file_path: sym.file_path,
                            line_start: sym.line_start,
                        });
                    }
                }
                _ => {}
            }

            hits.push(hit);
        }

        Ok(Json(hits))
    }
}

/// Admin GC controller
#[derive(Controller)]
#[controller(path = "/admin", state = AppState)]
pub struct AdminController {
    #[inject]
    db: Arc<db::Database>,
}

#[routes]
impl AdminController {
    #[post("/gc")]
    async fn gc(&self) -> Result<Json<crate::db::GcStats>, GitdocError> {
        let stats = self.db.gc_orphans().await?;
        Ok(Json(stats))
    }
}
