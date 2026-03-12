use r2e::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use gitdoc_api_types::requests::{SummarizeQuery, SummaryQuery};

use crate::AppState;
use crate::error::GitdocError;

#[derive(Serialize)]
pub struct SummarizeResponse {
    pub snapshot_id: i64,
    pub scope: String,
    pub content: String,
}

/// Response for GET /summary: either a single summary or a list of all summaries.
#[derive(Serialize)]
#[serde(untagged)]
pub enum SummaryResponse {
    Single(crate::db::SummaryRow),
    List(Vec<crate::db::SummaryRow>),
}

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct SummaryController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl SummaryController {
    #[post("/{snapshot_id}/summarize")]
    async fn summarize(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<SummarizeQuery>,
    ) -> Result<Json<SummarizeResponse>, GitdocError> {
        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| GitdocError::ServiceUnavailable("no LLM provider configured (set GITDOC_LLM_ENDPOINT)".into()))?;

        let content = crate::llm::generate_and_store_summary(
            llm_client.clone(),
            &self.db,
            snapshot_id,
            &q.scope,
        )
        .await
        .map_err(|e| GitdocError::Internal(e))?;

        Ok(Json(SummarizeResponse {
            snapshot_id,
            scope: q.scope,
            content,
        }))
    }

    #[get("/{snapshot_id}/summary")]
    async fn get_summary(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<SummaryQuery>,
    ) -> Result<Json<SummaryResponse>, GitdocError> {
        if let Some(scope) = &q.scope {
            let summary = self
                .db
                .get_summary(snapshot_id, scope)
                .await?
                .ok_or_else(|| GitdocError::NotFound(format!("no summary for scope '{scope}'. Call POST /summarize first.")))?;
            Ok(Json(SummaryResponse::Single(summary)))
        } else {
            let summaries = self.db.list_summaries(snapshot_id).await?;
            Ok(Json(SummaryResponse::List(summaries)))
        }
    }
}
