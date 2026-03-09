mod libs;
mod rules;
mod projects;
mod decisions;
mod patterns;
mod advise;

pub use libs::*;
pub use rules::*;
pub use projects::*;
pub use decisions::*;
pub use patterns::*;
pub use advise::*;

use serde::Serialize;

use gitdoc_api_types::requests::CompareLibsRequest;

use crate::embeddings::{self, EmbeddingProvider};
use crate::error::GitdocError;

#[derive(Serialize)]
pub struct DeletedResponse {
    pub deleted: bool,
}

/// Generate an embedding vector for the given text, if an embedder is available.
pub(crate) async fn maybe_embed(
    embedder: Option<&dyn EmbeddingProvider>,
    text: &str,
) -> Result<Option<pgvector::Vector>, GitdocError> {
    match embedder {
        Some(emb) if !text.trim().is_empty() => {
            let vec = emb.embed_query(text).await.map_err(GitdocError::Internal)?;
            Ok(Some(embeddings::to_pgvector(&vec)))
        }
        _ => Ok(None),
    }
}

#[derive(Serialize)]
pub struct CompareLibsResponse {
    pub comparison: String,
}

/// POST /architect/compare
pub async fn compare(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>,
    axum::Json(req): axum::Json<CompareLibsRequest>,
) -> Result<axum::Json<CompareLibsResponse>, crate::error::GitdocError> {
    let llm_client = state.llm_client.as_ref().ok_or_else(|| {
        crate::error::GitdocError::ServiceUnavailable("no LLM provider configured".into())
    })?;

    let comparison = crate::architect::compare_libs(
        &state.db,
        llm_client,
        &req.lib_ids,
        &req.criteria,
    ).await.map_err(crate::error::GitdocError::Internal)?;

    Ok(axum::Json(CompareLibsResponse { comparison }))
}

pub(crate) fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}
