use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

#[derive(Deserialize)]
pub struct CreatePatternRequest {
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub libs_involved: Option<Vec<String>>,
    pub pattern_text: String,
}

#[derive(Deserialize)]
pub struct ListPatternsQuery {
    pub category: Option<String>,
}

/// GET /architect/patterns
pub async fn list_patterns(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListPatternsQuery>,
) -> Result<Json<Vec<crate::db::ArchPatternRow>>, GitdocError> {
    let patterns = state.db.list_arch_patterns(q.category.as_deref()).await?;
    Ok(Json(patterns))
}

/// POST /architect/patterns
pub async fn create_pattern(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePatternRequest>,
) -> Result<Json<crate::db::ArchPatternRow>, GitdocError> {
    let category = req.category.as_deref().unwrap_or("");
    let description = req.description.as_deref().unwrap_or("");
    let libs = req.libs_involved.unwrap_or_default();

    let embed_text = format!("{} {} {} {}", req.name, category, description, req.pattern_text);

    let embedding = maybe_embed(state.embedder.as_deref(), &embed_text).await?;

    let id = state.db.create_arch_pattern(
        &req.name,
        category,
        description,
        &libs,
        &req.pattern_text,
        "manual",
        embedding,
    ).await?;

    let row = state.db.get_arch_pattern(id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("pattern vanished")))?;

    Ok(Json(row))
}

/// GET /architect/patterns/{id}
pub async fn get_pattern(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<crate::db::ArchPatternRow>, GitdocError> {
    let row = state.db.get_arch_pattern(id).await?
        .ok_or_else(|| GitdocError::NotFound(format!("pattern {id} not found")))?;
    Ok(Json(row))
}

/// DELETE /architect/patterns/{id}
pub async fn delete_pattern(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    let deleted = state.db.delete_arch_pattern(id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!("pattern {id} not found")));
    }
    Ok(Json(DeletedResponse { deleted: true }))
}
