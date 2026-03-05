use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;
use super::DeletedResponse;

#[derive(Deserialize)]
pub struct ListLibsQuery {
    pub category: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateLibRequest {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub version_hint: Option<String>,
    pub profile: Option<String>,
}

#[derive(Deserialize)]
pub struct GenerateLibProfileRequest {
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
}

/// GET /architect/libs
pub async fn list_libs(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<ListLibsQuery>,
) -> Result<Json<Vec<crate::db::LibProfileSummary>>, GitdocError> {
    let profiles = state.db.list_lib_profiles(q.category.as_deref()).await?;
    Ok(Json(profiles))
}

/// POST /architect/libs
pub async fn create_lib(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateLibRequest>,
) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
    let profile_text = crate::architect::parse_manual_profile(
        req.profile.as_deref().unwrap_or(""),
    );

    let embedding = if let Some(ref embedder) = state.embedder {
        if !profile_text.is_empty() {
            let vec = embedder
                .embed_query(&profile_text)
                .await
                .map_err(GitdocError::Internal)?;
            Some(embeddings::to_pgvector(&vec))
        } else {
            None
        }
    } else {
        None
    };

    state
        .db
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

    let row = state
        .db
        .get_lib_profile(&req.id)
        .await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("lib profile vanished")))?;

    Ok(Json(row))
}

/// GET /architect/libs/{id}
pub async fn get_lib(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
    let row = state
        .db
        .get_lib_profile(&id)
        .await?
        .ok_or_else(|| GitdocError::NotFound(format!("lib profile '{id}' not found")))?;
    Ok(Json(row))
}

/// DELETE /architect/libs/{id}
pub async fn delete_lib(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    let deleted = state.db.delete_lib_profile(&id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!("lib profile '{id}' not found")));
    }
    Ok(Json(DeletedResponse { deleted: true }))
}

/// POST /architect/libs/{id}/generate
pub async fn generate_lib_profile_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<GenerateLibProfileRequest>,
) -> Result<Json<crate::db::LibProfileRow>, GitdocError> {
    let llm_client = state.llm_client.as_ref().ok_or_else(|| {
        GitdocError::ServiceUnavailable("no LLM provider configured".into())
    })?;

    let _repo = state
        .db
        .get_repo(&req.repo_id)
        .await?
        .ok_or_else(|| GitdocError::NotFound(format!("repo '{}' not found", req.repo_id)))?;

    let snapshot_id = if let Some(sid) = req.snapshot_id {
        sid
    } else {
        let snapshots = state.db.list_snapshots(&req.repo_id).await?;
        snapshots
            .last()
            .ok_or_else(|| {
                GitdocError::NotFound(format!("no snapshots for repo '{}'", req.repo_id))
            })?
            .id
    };

    let existing = state.db.get_lib_profile(&id).await?;
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

    let embedder = state.embedder.as_deref();

    let row = crate::architect::generate_lib_profile(
        llm_client,
        embedder,
        &state.db,
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
