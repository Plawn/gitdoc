use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;

#[derive(Deserialize)]
pub struct SummarizeQuery {
    pub scope: String,
}

pub async fn summarize(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SummarizeQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let llm_client = state
        .llm_client
        .as_ref()
        .ok_or_else(|| GitdocError::ServiceUnavailable("no LLM provider configured (set GITDOC_LLM_ENDPOINT)".into()))?;

    let content = crate::llm::generate_and_store_summary(
        llm_client.clone(),
        &state.db,
        snapshot_id,
        &q.scope,
    )
    .await
    .map_err(|e| GitdocError::Internal(e))?;

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id,
        "scope": q.scope,
        "content": content,
    })))
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    pub scope: Option<String>,
}

pub async fn get_summary(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    if let Some(scope) = &q.scope {
        let summary = state
            .db
            .get_summary(snapshot_id, scope)
            .await?
            .ok_or_else(|| GitdocError::NotFound(format!("no summary for scope '{scope}'. Call POST /summarize first.")))?;
        Ok(Json(serde_json::json!(summary)))
    } else {
        let summaries = state.db.list_summaries(snapshot_id).await?;
        Ok(Json(serde_json::json!(summaries)))
    }
}
