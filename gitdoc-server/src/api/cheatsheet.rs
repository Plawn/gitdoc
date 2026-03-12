use r2e::prelude::*;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

use gitdoc_api_types::requests::{GenerateCheatsheetRequest, PatchListQuery};

use crate::AppState;
use crate::error::GitdocError;

#[derive(Serialize)]
pub struct CheatsheetResponse {
    pub repo_id: String,
    pub content: String,
    pub model: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct GenerateCheatsheetResponse {
    pub repo_id: String,
    pub patch_id: i64,
    pub content: String,
    pub model: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Controller)]
#[controller(path = "/repos", state = AppState)]
pub struct CheatsheetController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
}

#[routes]
impl CheatsheetController {
    /// POST /repos/{repo_id}/cheatsheet
    #[post("/{repo_id}/cheatsheet")]
    async fn generate_cheatsheet_handler(
        &self,
        Path(repo_id): Path<String>,
        Json(req): Json<GenerateCheatsheetRequest>,
    ) -> Result<Json<GenerateCheatsheetResponse>, GitdocError> {
        let llm_client = self.llm_client.as_ref().ok_or_else(|| {
            GitdocError::ServiceUnavailable("no LLM provider configured".into())
        })?;

        let trigger = req.trigger.as_deref().unwrap_or("manual");

        let patch_id = crate::cheatsheet::generate_and_store_cheatsheet(
            llm_client.clone(),
            &self.db,
            &repo_id,
            Some(req.snapshot_id),
            trigger,
            None,
            self.embedder.as_deref(),
        )
        .await
        .map_err(GitdocError::Internal)?;

        let cs = self
            .db
            .get_cheatsheet(&repo_id)
            .await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("cheatsheet vanished after creation")))?;

        Ok(Json(GenerateCheatsheetResponse {
            repo_id: cs.repo_id,
            patch_id,
            content: cs.content,
            model: cs.model,
            updated_at: cs.updated_at,
        }))
    }

    /// GET /repos/{repo_id}/cheatsheet
    #[get("/{repo_id}/cheatsheet")]
    async fn get_cheatsheet_handler(
        &self,
        Path(repo_id): Path<String>,
    ) -> Result<Json<CheatsheetResponse>, GitdocError> {
        let cs = self
            .db
            .get_cheatsheet(&repo_id)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("no cheatsheet for repo '{repo_id}'")))?;

        Ok(Json(CheatsheetResponse {
            repo_id: cs.repo_id,
            content: cs.content,
            model: cs.model,
            updated_at: cs.updated_at,
        }))
    }

    /// POST /repos/{repo_id}/cheatsheet/stream
    #[post("/{repo_id}/cheatsheet/stream")]
    async fn stream_generate_cheatsheet_handler(
        &self,
        Path(repo_id): Path<String>,
        Json(req): Json<GenerateCheatsheetRequest>,
    ) -> Result<Sse<ReceiverStream<Result<SseEvent, Infallible>>>, GitdocError> {
        let llm_client = self.llm_client.as_ref().ok_or_else(|| {
            GitdocError::ServiceUnavailable("no LLM provider configured".into())
        })?.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(16);
        let trigger = req.trigger.unwrap_or_else(|| "auto".into());
        let snapshot_id = req.snapshot_id;
        let db = self.db.clone();
        let embedder = self.embedder.clone();

        tokio::spawn(async move {
            let send_event = |stage: &str, message: &str, patch_id: Option<i64>| {
                let mut obj = serde_json::json!({ "stage": stage, "message": message });
                if let Some(pid) = patch_id {
                    obj["patch_id"] = serde_json::json!(pid);
                }
                SseEvent::default().data(obj.to_string())
            };

            let _ = tx.send(Ok(send_event("gathering", "Loading repo structure...", None))).await;

            match crate::cheatsheet::generate_and_store_cheatsheet(
                llm_client,
                &db,
                &repo_id,
                Some(snapshot_id),
                &trigger,
                None,
                embedder.as_deref(),
            ).await {
                Ok(patch_id) => {
                    let _ = tx.send(Ok(send_event("generating", "Calling LLM...", None))).await;
                    let _ = tx.send(Ok(send_event("done", "Cheatsheet generated", Some(patch_id)))).await;
                }
                Err(e) => {
                    let _ = tx.send(Ok(send_event("error", &format!("Generation failed: {e}"), None))).await;
                }
            }
        });

        Ok(Sse::new(ReceiverStream::new(rx)))
    }

    /// GET /repos/{repo_id}/cheatsheet/patches
    #[get("/{repo_id}/cheatsheet/patches")]
    async fn list_patches_handler(
        &self,
        Path(repo_id): Path<String>,
        Query(q): Query<PatchListQuery>,
    ) -> Result<Json<Vec<crate::db::CheatsheetPatchMeta>>, GitdocError> {
        let limit = q.limit.unwrap_or(20);
        let offset = q.offset.unwrap_or(0);

        let patches = self
            .db
            .list_cheatsheet_patches(&repo_id, limit, offset)
            .await?;

        Ok(Json(patches))
    }

    /// GET /repos/{repo_id}/cheatsheet/patches/{patch_id}
    #[get("/{repo_id}/cheatsheet/patches/{patch_id}")]
    async fn get_patch_handler(
        &self,
        Path((repo_id, patch_id)): Path<(String, i64)>,
    ) -> Result<Json<crate::db::CheatsheetPatchRow>, GitdocError> {
        let patch = self
            .db
            .get_cheatsheet_patch(&repo_id, patch_id)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("patch {patch_id} not found for repo '{repo_id}'")))?;

        Ok(Json(patch))
    }
}
