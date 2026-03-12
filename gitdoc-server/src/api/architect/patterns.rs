use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::{CreatePatternRequest, ListPatternsQuery};

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

#[derive(Controller)]
#[controller(path = "/architect", state = AppState)]
pub struct ArchitectPatternController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
}

#[routes]
impl ArchitectPatternController {
    /// GET /architect/patterns
    #[get("/patterns")]
    async fn list_patterns(
        &self,
        Query(q): Query<ListPatternsQuery>,
    ) -> Result<Json<Vec<crate::db::ArchPatternRow>>, GitdocError> {
        let patterns = self.db.list_arch_patterns(q.category.as_deref()).await?;
        Ok(Json(patterns))
    }

    /// POST /architect/patterns
    #[post("/patterns")]
    async fn create_pattern(
        &self,
        Json(req): Json<CreatePatternRequest>,
    ) -> Result<Json<crate::db::ArchPatternRow>, GitdocError> {
        let category = req.category.as_deref().unwrap_or("");
        let description = req.description.as_deref().unwrap_or("");
        let libs = req.libs_involved.unwrap_or_default();

        let embed_text = format!("{} {} {} {}", req.name, category, description, req.pattern_text);

        let embedding = maybe_embed(self.embedder.as_deref(), &embed_text).await?;

        let id = self.db.create_arch_pattern(
            &req.name,
            category,
            description,
            &libs,
            &req.pattern_text,
            "manual",
            embedding,
        ).await?;

        let row = self.db.get_arch_pattern(id).await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("pattern vanished")))?;

        Ok(Json(row))
    }

    /// GET /architect/patterns/{id}
    #[get("/patterns/{id}")]
    async fn get_pattern(
        &self,
        Path(id): Path<i64>,
    ) -> Result<Json<crate::db::ArchPatternRow>, GitdocError> {
        let row = self.db.get_arch_pattern(id).await?
            .ok_or_else(|| GitdocError::NotFound(format!("pattern {id} not found")))?;
        Ok(Json(row))
    }

    /// DELETE /architect/patterns/{id}
    #[delete("/patterns/{id}")]
    async fn delete_pattern(
        &self,
        Path(id): Path<i64>,
    ) -> Result<Json<DeletedResponse>, GitdocError> {
        let deleted = self.db.delete_arch_pattern(id).await?;
        if !deleted {
            return Err(GitdocError::NotFound(format!("pattern {id} not found")));
        }
        Ok(Json(DeletedResponse { deleted: true }))
    }
}
