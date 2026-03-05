use axum::{
    Json,
    extract::{Path, Query, State},
    response::sse::{Event, Sse},
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

use crate::AppState;
use crate::error::GitdocError;

#[derive(Deserialize)]
pub struct GenerateCheatsheetRequest {
    pub snapshot_id: i64,
    pub trigger: Option<String>,
}

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

#[derive(Deserialize)]
pub struct PatchListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// POST /repos/{repo_id}/cheatsheet — generate or update the cheatsheet
pub async fn generate_cheatsheet_handler(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<GenerateCheatsheetRequest>,
) -> Result<Json<GenerateCheatsheetResponse>, GitdocError> {
    let llm_client = state.llm_client.as_ref().ok_or_else(|| {
        GitdocError::ServiceUnavailable("no LLM provider configured".into())
    })?;

    let trigger = req.trigger.as_deref().unwrap_or("manual");

    let patch_id = crate::cheatsheet::generate_and_store_cheatsheet(
        llm_client.clone(),
        &state.db,
        &repo_id,
        Some(req.snapshot_id),
        trigger,
        None,
    )
    .await
    .map_err(GitdocError::Internal)?;

    let cs = state
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

/// GET /repos/{repo_id}/cheatsheet — get the current cheatsheet
pub async fn get_cheatsheet_handler(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<CheatsheetResponse>, GitdocError> {
    let cs = state
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

/// GET /repos/{repo_id}/cheatsheet/patches — list patch history (meta only)
pub async fn list_patches_handler(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(q): Query<PatchListQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let limit = q.limit.unwrap_or(20);
    let offset = q.offset.unwrap_or(0);

    let patches = state
        .db
        .list_cheatsheet_patches(&repo_id, limit, offset)
        .await?;

    Ok(Json(serde_json::to_value(&patches).unwrap_or_default()))
}

/// GET /repos/{repo_id}/cheatsheet/patches/{patch_id} — get full patch
pub async fn get_patch_handler(
    State(state): State<Arc<AppState>>,
    Path((repo_id, patch_id)): Path<(String, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let patch = state
        .db
        .get_cheatsheet_patch(&repo_id, patch_id)
        .await?
        .ok_or_else(|| GitdocError::NotFound(format!("patch {patch_id} not found for repo '{repo_id}'")))?;

    Ok(Json(serde_json::to_value(&patch).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct StreamGenerateCheatsheetRequest {
    pub snapshot_id: i64,
    pub trigger: Option<String>,
}

/// POST /repos/{repo_id}/cheatsheet/stream — generate cheatsheet with SSE progress
pub async fn stream_generate_cheatsheet_handler(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<StreamGenerateCheatsheetRequest>,
) -> Result<Sse<ReceiverStream<Result<Event, Infallible>>>, GitdocError> {
    let llm_client = state.llm_client.as_ref().ok_or_else(|| {
        GitdocError::ServiceUnavailable("no LLM provider configured".into())
    })?.clone();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);
    let trigger = req.trigger.unwrap_or_else(|| "auto".into());
    let snapshot_id = req.snapshot_id;
    let db = state.db.clone();

    tokio::spawn(async move {
        let send_event = |stage: &str, message: &str, patch_id: Option<i64>| {
            let mut obj = serde_json::json!({ "stage": stage, "message": message });
            if let Some(pid) = patch_id {
                obj["patch_id"] = serde_json::json!(pid);
            }
            Event::default().data(obj.to_string())
        };

        let _ = tx.send(Ok(send_event("gathering", "Loading repo structure...", None))).await;

        match crate::cheatsheet::generate_and_store_cheatsheet(
            llm_client,
            &db,
            &repo_id,
            Some(snapshot_id),
            &trigger,
            None,
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
