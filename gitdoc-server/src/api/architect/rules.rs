use r2e::prelude::*;
use std::sync::Arc;

use gitdoc_api_types::requests::{ListRulesQuery, UpsertRuleRequest};

use crate::AppState;
use crate::error::GitdocError;
use super::{DeletedResponse, maybe_embed};

#[derive(Controller)]
#[controller(path = "/architect", state = AppState)]
pub struct ArchitectRuleController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
}

#[routes]
impl ArchitectRuleController {
    /// GET /architect/rules
    #[get("/rules")]
    async fn list_rules(
        &self,
        Query(q): Query<ListRulesQuery>,
    ) -> Result<Json<Vec<crate::db::StackRuleRow>>, GitdocError> {
        let rules = self
            .db
            .list_stack_rules(q.rule_type.as_deref(), q.subject.as_deref())
            .await?;
        Ok(Json(rules))
    }

    /// POST /architect/rules
    #[post("/rules")]
    async fn upsert_rule(
        &self,
        Json(req): Json<UpsertRuleRequest>,
    ) -> Result<Json<crate::db::StackRuleRow>, GitdocError> {
        let embedding = maybe_embed(self.embedder.as_deref(), &req.content).await?;

        let rule_id = self
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

        let row = self
            .db
            .get_stack_rule(rule_id)
            .await?
            .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("stack rule vanished")))?;

        Ok(Json(row))
    }

    /// DELETE /architect/rules/{id}
    #[delete("/rules/{id}")]
    async fn delete_rule(
        &self,
        Path(id): Path<i64>,
    ) -> Result<Json<DeletedResponse>, GitdocError> {
        let deleted = self.db.delete_stack_rule(id).await?;
        if !deleted {
            return Err(GitdocError::NotFound(format!("stack rule {id} not found")));
        }
        Ok(Json(DeletedResponse { deleted: true }))
    }
}
