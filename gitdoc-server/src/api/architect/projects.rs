use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

use gitdoc_api_types::requests::CreateProjectProfileRequest;

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

/// GET /architect/projects
pub async fn list_projects(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::db::ProjectProfileSummary>>, GitdocError> {
    let profiles = state.db.list_project_profiles().await?;
    Ok(Json(profiles))
}

/// POST /architect/projects
pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectProfileRequest>,
) -> Result<Json<crate::db::ProjectProfileRow>, GitdocError> {
    let stack = req.stack.unwrap_or(serde_json::json!([]));
    let description = req.description.as_deref().unwrap_or("");
    let constraints = req.constraints.as_deref().unwrap_or("");
    let code_style = req.code_style.as_deref().unwrap_or("");

    // Build embedding content
    let embed_text = format!(
        "{} {} {} {} {}",
        req.name, description, serde_json::to_string(&stack).unwrap_or_default(), constraints, code_style
    );

    let embedding = maybe_embed(state.embedder.as_deref(), &embed_text).await?;

    state.db.upsert_project_profile(
        &req.id,
        req.repo_id.as_deref(),
        &req.name,
        description,
        &stack,
        constraints,
        code_style,
        embedding,
    ).await?;

    let row = state.db.get_project_profile(&req.id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("project profile vanished")))?;

    Ok(Json(row))
}

/// GET /architect/projects/{id}
pub async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::db::ProjectProfileRow>, GitdocError> {
    let row = state.db.get_project_profile(&id).await?
        .ok_or_else(|| GitdocError::NotFound(format!("project profile '{id}' not found")))?;
    Ok(Json(row))
}

/// DELETE /architect/projects/{id}
pub async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    let deleted = state.db.delete_project_profile(&id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!("project profile '{id}' not found")));
    }
    Ok(Json(DeletedResponse { deleted: true }))
}
