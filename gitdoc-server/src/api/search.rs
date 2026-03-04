use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;

#[derive(Deserialize)]
pub struct DocSearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SymbolSearchQuery {
    pub q: String,
    pub kind: Option<String>,
    pub visibility: Option<String>,
    pub limit: Option<usize>,
}

pub async fn search_docs(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<DocSearchQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    if q.q.is_empty() {
        return Err(GitdocError::BadRequest("q must be non-empty".into()));
    }

    let file_ids = state.db.get_file_ids_for_snapshot(snapshot_id).await?;
    let search = Arc::clone(&state.search);
    let limit = q.limit.unwrap_or(10);
    let query_str = q.q;

    let result = tokio::task::spawn_blocking(move || {
        search.search_docs(&query_str, &file_ids, limit)
    })
    .await??;

    Ok(Json(serde_json::json!(result)))
}

pub async fn search_symbols(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SymbolSearchQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    if q.q.is_empty() {
        return Err(GitdocError::BadRequest("q must be non-empty".into()));
    }

    let file_ids = state.db.get_file_ids_for_snapshot(snapshot_id).await?;
    let search = Arc::clone(&state.search);
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

    Ok(Json(serde_json::json!(result)))
}

// --- Semantic search ---

#[derive(Deserialize)]
pub struct SemanticSearchQuery {
    pub q: String,
    pub scope: Option<String>,
    pub limit: Option<usize>,
}

#[derive(serde::Serialize)]
struct SemanticHit {
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
struct DocMeta {
    file_path: String,
    title: Option<String>,
}

#[derive(serde::Serialize)]
struct SymbolMeta {
    name: String,
    qualified_name: String,
    kind: String,
    signature: String,
    file_path: String,
    line_start: i64,
}

pub async fn search_semantic(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SemanticSearchQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    if q.q.is_empty() {
        return Err(GitdocError::BadRequest("q must be non-empty".into()));
    }

    let embedder = state.embedder.as_ref()
        .ok_or_else(|| GitdocError::ServiceUnavailable("no embedding provider configured".into()))?;
    let embedder = Arc::clone(embedder);

    let db = Arc::clone(&state.db);
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

    let docs = db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_default();
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

    Ok(Json(serde_json::json!(hits)))
}

pub async fn gc(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let stats = state.db.gc_orphans().await?;
    Ok(Json(serde_json::json!(stats)))
}
