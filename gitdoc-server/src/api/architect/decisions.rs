use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;
use super::DeletedResponse;

#[derive(Deserialize)]
pub struct CreateDecisionRequest {
    pub project_profile_id: Option<String>,
    pub title: String,
    pub context: Option<String>,
    pub choice: String,
    pub alternatives: Option<String>,
    pub reasoning: Option<String>,
}

#[derive(Deserialize)]
pub struct ListDecisionsQuery {
    pub project_profile_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateDecisionRequest {
    pub outcome: Option<String>,
    pub status: Option<String>,
}

/// POST /architect/decisions
pub async fn create_decision(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDecisionRequest>,
) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
    let context = req.context.as_deref().unwrap_or("");
    let alternatives = req.alternatives.as_deref().unwrap_or("");
    let reasoning = req.reasoning.as_deref().unwrap_or("");

    let embed_text = format!("{} {} {} {} {}", req.title, context, req.choice, reasoning, alternatives);

    let embedding = if let Some(ref embedder) = state.embedder {
        let vec = embedder.embed_query(&embed_text).await.map_err(GitdocError::Internal)?;
        Some(embeddings::to_pgvector(&vec))
    } else {
        None
    };

    let id = state.db.create_arch_decision(
        req.project_profile_id.as_deref(),
        &req.title,
        context,
        &req.choice,
        alternatives,
        reasoning,
        embedding,
    ).await?;

    let row = state.db.get_arch_decision(id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("decision vanished")))?;

    Ok(Json(row))
}

/// GET /architect/decisions
pub async fn list_decisions(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListDecisionsQuery>,
) -> Result<Json<Vec<crate::db::ArchDecisionRow>>, GitdocError> {
    let decisions = state.db.list_arch_decisions(
        q.project_profile_id.as_deref(),
        q.status.as_deref(),
    ).await?;
    Ok(Json(decisions))
}

/// GET /architect/decisions/{id}
pub async fn get_decision(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
    let row = state.db.get_arch_decision(id).await?
        .ok_or_else(|| GitdocError::NotFound(format!("decision {id} not found")))?;
    Ok(Json(row))
}

/// PUT /architect/decisions/{id}
pub async fn update_decision(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDecisionRequest>,
) -> Result<Json<crate::db::ArchDecisionRow>, GitdocError> {
    let existing = state.db.get_arch_decision(id).await?
        .ok_or_else(|| GitdocError::NotFound(format!("decision {id} not found")))?;

    let new_outcome = req.outcome.as_deref().or(existing.outcome.as_deref());
    let new_status = req.status.as_deref().unwrap_or(&existing.status);

    let embed_text = format!(
        "{} {} {} {} {} {}",
        existing.title, existing.context, existing.choice, existing.reasoning, existing.alternatives,
        new_outcome.unwrap_or("")
    );

    let embedding = if let Some(ref embedder) = state.embedder {
        let vec = embedder.embed_query(&embed_text).await.map_err(GitdocError::Internal)?;
        Some(embeddings::to_pgvector(&vec))
    } else {
        None
    };

    let updated = state.db.update_arch_decision(
        id,
        req.outcome.as_deref(),
        Some(new_status),
        embedding,
    ).await?;

    if !updated {
        return Err(GitdocError::NotFound(format!("decision {id} not found")));
    }

    let row = state.db.get_arch_decision(id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("decision vanished")))?;

    Ok(Json(row))
}

/// DELETE /architect/decisions/{id}
pub async fn delete_decision(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    let deleted = state.db.delete_arch_decision(id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!("decision {id} not found")));
    }
    Ok(Json(DeletedResponse { deleted: true }))
}
