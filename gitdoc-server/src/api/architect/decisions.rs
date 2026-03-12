use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::{CreateDecisionRequest, ListDecisionsQuery, UpdateDecisionRequest};

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

#[derive(Controller)]
#[controller(path = "/architect", state = AppState)]
pub struct ArchitectDecisionController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
}

#[routes]
impl ArchitectDecisionController {
    /// POST /architect/decisions
    #[post("/decisions")]
    async fn create_decision(
        &self,
        Json(req): Json<CreateDecisionRequest>,
    ) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
        let context = req.context.as_deref().unwrap_or("");
        let alternatives = req.alternatives.as_deref().unwrap_or("");
        let reasoning = req.reasoning.as_deref().unwrap_or("");

        let embed_text = format!("{} {} {} {} {}", req.title, context, req.choice, reasoning, alternatives);

        let embedding = maybe_embed(self.embedder.as_deref(), &embed_text).await?;

        let id = self.db.create_arch_decision(
            req.project_profile_id.as_deref(),
            &req.title,
            context,
            &req.choice,
            alternatives,
            reasoning,
            embedding,
        ).await?;

        let row = self.db.get_arch_decision(id).await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("decision vanished")))?;

        Ok(Json(row))
    }

    /// GET /architect/decisions
    #[get("/decisions")]
    async fn list_decisions(
        &self,
        Query(q): Query<ListDecisionsQuery>,
    ) -> Result<Json<Vec<crate::db::ArchDecisionRow>>, GitdocError> {
        let decisions = self.db.list_arch_decisions(
            q.project_profile_id.as_deref(),
            q.status.as_deref(),
        ).await?;
        Ok(Json(decisions))
    }

    /// GET /architect/decisions/{id}
    #[get("/decisions/{id}")]
    async fn get_decision(
        &self,
        Path(id): Path<i64>,
    ) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
        let row = self.db.get_arch_decision(id).await?
            .ok_or_else(|| GitdocError::NotFound(format!("decision {id} not found")))?;
        Ok(Json(row))
    }

    /// PUT /architect/decisions/{id}
    #[put("/decisions/{id}")]
    async fn update_decision(
        &self,
        Path(id): Path<i64>,
        Json(req): Json<UpdateDecisionRequest>,
    ) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
        let existing = self.db.get_arch_decision(id).await?
            .ok_or_else(|| GitdocError::NotFound(format!("decision {id} not found")))?;

        let new_outcome = req.outcome.as_deref().or(existing.outcome.as_deref());
        let new_status = req.status.as_deref().unwrap_or(&existing.status);

        let embed_text = format!(
            "{} {} {} {} {} {}",
            existing.title, existing.context, existing.choice, existing.reasoning, existing.alternatives,
            new_outcome.unwrap_or("")
        );

        let embedding = maybe_embed(self.embedder.as_deref(), &embed_text).await?;

        let updated = self.db.update_arch_decision(
            id,
            req.outcome.as_deref(),
            Some(new_status),
            embedding,
        ).await?;

        if !updated {
            return Err(GitdocError::NotFound(format!("decision {id} not found")));
        }

        let row = self.db.get_arch_decision(id).await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("decision vanished")))?;

        Ok(Json(row))
    }

    /// DELETE /architect/decisions/{id}
    #[delete("/decisions/{id}")]
    async fn delete_decision(
        &self,
        Path(id): Path<i64>,
    ) -> Result<Json<DeletedResponse>, GitdocError> {
        let deleted = self.db.delete_arch_decision(id).await?;
        if !deleted {
            return Err(GitdocError::NotFound(format!("decision {id} not found")));
        }
        Ok(Json(DeletedResponse { deleted: true }))
    }
}
