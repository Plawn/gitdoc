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

use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::CompareLibsRequest;
use gitdoc_api_types::responses::{
    DeleteResponse as DeletedResponse,
    CompareLibsResponse,
};

use crate::embeddings::{self, EmbeddingProvider};
use crate::error::GitdocError;

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

#[derive(Controller)]
#[controller(path = "/architect", state = crate::AppState)]
pub struct ArchitectCompareController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl ArchitectCompareController {
    /// POST /architect/compare
    #[post("/compare")]
    async fn compare(
        &self,
        Json(req): Json<CompareLibsRequest>,
    ) -> Result<Json<CompareLibsResponse>, crate::error::GitdocError> {
        let llm_client = self.llm_client.as_ref().ok_or_else(|| {
            crate::error::GitdocError::ServiceUnavailable("no LLM provider configured".into())
        })?;

        let comparison = crate::architect::compare_libs(
            &self.db,
            llm_client,
            &req.lib_ids,
            &req.criteria,
        ).await.map_err(crate::error::GitdocError::Internal)?;

        Ok(Json(CompareLibsResponse { comparison }))
    }
}

pub(crate) fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}
