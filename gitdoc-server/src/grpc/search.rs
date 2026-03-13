use r2e::prelude::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::proto;
use crate::AppState;

#[derive(Controller)]
#[controller(state = AppState)]
pub struct SearchGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    search: Arc<crate::search::SearchIndex>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
}

#[grpc_routes(proto::search_service_server::SearchService)]
impl SearchGrpcService {
    async fn search_docs(
        &self,
        request: Request<proto::SearchDocsRequest>,
    ) -> Result<Response<proto::SearchDocsResponse>, Status> {
        let req = request.into_inner();
        if req.q.is_empty() {
            return Err(Status::invalid_argument("q must be non-empty"));
        }
        let file_ids = self
            .db
            .get_file_ids_for_snapshot(req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let search = self.search.clone();
        let limit = if req.limit == 0 { 10 } else { req.limit as usize };
        let query_str = req.q;
        let result = tokio::task::spawn_blocking(move || {
            search.search_docs(&query_str, &file_ids, limit)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::SearchDocsResponse {
            results: result.into_iter().map(Into::into).collect(),
        }))
    }

    async fn search_symbols(
        &self,
        request: Request<proto::SearchSymbolsRequest>,
    ) -> Result<Response<proto::SearchSymbolsResponse>, Status> {
        let req = request.into_inner();
        if req.q.is_empty() {
            return Err(Status::invalid_argument("q must be non-empty"));
        }
        let file_ids = self
            .db
            .get_file_ids_for_snapshot(req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let search = self.search.clone();
        let limit = if req.limit == 0 { 10 } else { req.limit as usize };
        let query_str = req.q;
        let kind = if req.kind.is_empty() { None } else { Some(req.kind) };
        let visibility = if req.visibility.is_empty() { None } else { Some(req.visibility) };
        let result = tokio::task::spawn_blocking(move || {
            search.search_symbols(&query_str, &file_ids, kind.as_deref(), visibility.as_deref(), limit)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::SearchSymbolsResponse {
            results: result.into_iter().map(Into::into).collect(),
        }))
    }

    async fn semantic_search(
        &self,
        request: Request<proto::SemanticSearchRequest>,
    ) -> Result<Response<proto::SemanticSearchResponse>, Status> {
        let req = request.into_inner();
        if req.q.is_empty() {
            return Err(Status::invalid_argument("q must be non-empty"));
        }
        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| Status::unavailable("no embedding provider configured"))?
            .clone();

        let limit = if req.limit == 0 { 10 } else { req.limit as usize };
        let scope = if req.scope.is_empty() {
            "all".to_string()
        } else {
            req.scope
        };

        let query_vec = embedder
            .embed_query(&req.q)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let file_ids = self
            .db
            .get_file_ids_for_snapshot(req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let query_pgvec = crate::embeddings::to_pgvector(&query_vec);
        let search_results = self
            .db
            .search_embeddings_by_vector(&query_pgvec, &file_ids, &scope, limit as i64)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let docs = self
            .db
            .list_docs_for_snapshot(req.snapshot_id)
            .await
            .unwrap_or_default();

        let mut hits: Vec<proto::SemanticSearchResult> = Vec::with_capacity(search_results.len());
        for r in &search_results {
            let mut hit = proto::SemanticSearchResult {
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
                        hit.doc = Some(proto::SemanticDocHit {
                            file_path: doc.file_path.clone(),
                            title: doc.title.clone().unwrap_or_default(),
                        });
                    }
                }
                "symbol" => {
                    if let Ok(Some(sym)) = self.db.get_symbol_by_id(r.source_id).await {
                        hit.symbol = Some(proto::SemanticSymbolHit {
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

        Ok(Response::new(proto::SemanticSearchResponse {
            results: hits,
        }))
    }

    async fn garbage_collect(
        &self,
        _request: Request<proto::GarbageCollectRequest>,
    ) -> Result<Response<proto::GarbageCollectResponse>, Status> {
        let stats = self
            .db
            .gc_orphans()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(stats.into()))
    }
}
