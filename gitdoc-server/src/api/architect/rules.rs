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
pub struct ListRulesQuery {
    pub rule_type: Option<String>,
    pub subject: Option<String>,
}

#[derive(Deserialize)]
pub struct UpsertRuleRequest {
    pub id: Option<i64>,
    pub rule_type: String,
    pub subject: String,
    pub content: String,
    pub lib_profile_id: Option<String>,
    pub priority: Option<i32>,
}

/// GET /architect/rules
pub async fn list_rules(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListRulesQuery>,
) -> Result<Json<Vec<crate::db::StackRuleRow>>, GitdocError> {
    let rules = state
        .db
        .list_stack_rules(q.rule_type.as_deref(), q.subject.as_deref())
        .await?;
    Ok(Json(rules))
}

/// POST /architect/rules
pub async fn upsert_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpsertRuleRequest>,
) -> Result<Json<crate::db::StackRuleRow>, GitdocError> {
    let embedding = maybe_embed(state.embedder.as_deref(), &req.content).await?;

    let rule_id = state
        .db
        .upsert_stack_rule(
            req.id,
            &req.rule_type,
            &req.subject,
            &req.content,
            req.lib_profile_id.as_deref(),
            req.priority.unwrap_or(0),
            embedding,
        )
        .await?;

    let row = state
        .db
        .get_stack_rule(rule_id)
        .await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("stack rule vanished")))?;

    Ok(Json(row))
}

/// DELETE /architect/rules/{id}
pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    let deleted = state.db.delete_stack_rule(id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!("stack rule {id} not found")));
    }
    Ok(Json(DeletedResponse { deleted: true }))
}
