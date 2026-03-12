use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::{ListLibsQuery, CreateLibRequest, GenerateLibProfileRequest};

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

#[derive(Controller)]
#[controller(path = "/architect", state = AppState)]
pub struct ArchitectLibController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl ArchitectLibController {
    /// GET /architect/libs
    #[get("/libs")]
    async fn list_libs(
        &self,
        Query(q): Query<ListLibsQuery>,
    ) -> Result<Json<Vec<crate::db::LibProfileSummary>>, GitdocError> {
        let profiles = self.db.list_lib_profiles(q.category.as_deref()).await?;
        Ok(Json(profiles))
    }

    /// POST /architect/libs
    #[post("/libs")]
    async fn create_lib(
        &self,
        Json(req): Json<CreateLibRequest>,
    ) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
        let profile_text = req.profile.as_deref().unwrap_or("").to_string();

        let embedding = maybe_embed(self.embedder.as_deref(), &profile_text).await?;

        self.db
            .upsert_lib_profile(
                &req.id,
                &req.name,
                None,
                req.category.as_deref().unwrap_or(""),
                req.version_hint.as_deref().unwrap_or(""),
                &profile_text,
                "manual",
                "",
                embedding,
            )
            .await?;

        let row = self
            .db
            .get_lib_profile(&req.id)
            .await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("lib profile vanished")))?;

        Ok(Json(row))
    }

    /// GET /architect/libs/{id}
    #[get("/libs/{id}")]
    async fn get_lib(
        &self,
        Path(id): Path<String>,
    ) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
        let row = self
            .db
            .get_lib_profile(&id)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("lib profile '{id}' not found")))?;
        Ok(Json(row))
    }

    /// DELETE /architect/libs/{id}
    #[delete("/libs/{id}")]
    async fn delete_lib(
        &self,
        Path(id): Path<String>,
    ) -> Result<Json<DeletedResponse>, GitdocError> {
        let deleted = self.db.delete_lib_profile(&id).await?;
        if !deleted {
            return Err(GitdocError::NotFound(format!("lib profile '{id}' not found")));
        }
        Ok(Json(DeletedResponse { deleted: true }))
    }

    /// POST /architect/libs/{id}/generate
    #[post("/libs/{id}/generate")]
    async fn generate_lib_profile_handler(
        &self,
        Path(id): Path<String>,
        Json(req): Json<GenerateLibProfileRequest>,
    ) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
        let llm_client = self.llm_client.as_ref().ok_or_else(|| {
            GitdocError::ServiceUnavailable("no LLM provider configured".into())
        })?;

        let _repo = self
            .db
            .get_repo(&req.repo_id)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("repo '{}' not found", req.repo_id)))?;

        let snapshot_id = if let Some(sid) = req.snapshot_id {
            sid
        } else {
            let snapshots = self.db.list_snapshots(&req.repo_id).await?;
            snapshots
                .last()
                .ok_or_else(|| {
                    GitdocError::NotFound(format!("no snapshots for repo '{}'", req.repo_id))
                })?
                .id
        };

        let existing = self.db.get_lib_profile(&id).await?;
        let lib_name = existing
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or(&id);
        let category = existing
            .as_ref()
            .map(|p| p.category.as_str())
            .unwrap_or("");
        let version_hint = existing
            .as_ref()
            .map(|p| p.version_hint.as_str())
            .unwrap_or("");

        let embedder = self.embedder.as_deref();

        let row = crate::architect::generate_lib_profile(
            llm_client,
            embedder,
            &self.db,
            &id,
            lib_name,
            &req.repo_id,
            snapshot_id,
            category,
            version_hint,
        )
        .await
        .map_err(GitdocError::Internal)?;

        Ok(Json(row))
    }
}
