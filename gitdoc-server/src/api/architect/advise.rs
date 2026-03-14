use r2e::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use gitdoc_api_types::requests::AdviseRequest;

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;
use crate::llm_executor::{LlmExecutor, PROMPT_ARCHITECT_ADVISE};
use super::truncate_text;

/// JSON shape matches `gitdoc_api_types::responses::AdviseResponse`.
/// Uses `crate::db::ArchitectSearchResult` directly since it serializes identically.
#[derive(Serialize)]
pub struct AdviseResponse {
    pub answer: String,
    pub relevant_libs: Vec<crate::db::ArchitectSearchResult>,
    pub relevant_rules: Vec<crate::db::ArchitectSearchResult>,
}

#[derive(Controller)]
#[controller(path = "/architect", state = AppState)]
pub struct ArchitectAdviseController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl ArchitectAdviseController {
    /// POST /architect/advise
    #[post("/advise")]
    async fn advise(
        &self,
        Json(req): Json<AdviseRequest>,
    ) -> Result<Json<AdviseResponse>, GitdocError> {
        let llm_client = self.llm_client.as_ref().ok_or_else(|| {
            GitdocError::ServiceUnavailable("no LLM provider configured".into())
        })?;

        let embedder = self.embedder.as_ref().ok_or_else(|| {
            GitdocError::ServiceUnavailable("no embedding provider configured".into())
        })?;

        let limit = req.limit.unwrap_or(5);

        let query_vec = embedder
            .embed_query(&req.question)
            .await
            .map_err(GitdocError::Internal)?;
        let query_pgvec = embeddings::to_pgvector(&query_vec);

        let results = self.db.search_architect_by_vector(&query_pgvec, limit).await?;

        let mut relevant_libs: Vec<crate::db::ArchitectSearchResult> = Vec::new();
        let mut relevant_rules: Vec<crate::db::ArchitectSearchResult> = Vec::new();
        let mut context = String::new();

        use crate::db::ArchitectResultKind;
        for r in results {
            match r.kind {
                ArchitectResultKind::LibProfile => {
                    context.push_str(&format!("### Library: {}\n{}\n\n", r.id, r.text));
                    relevant_libs.push(r);
                }
                ArchitectResultKind::StackRule => {
                    context.push_str(&format!("### Stack Rule #{}\n{}\n\n", r.id, r.text));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Cheatsheet => {
                    context.push_str(&format!("### Repo Cheatsheet ({})\n{}\n\n", r.id, truncate_text(&r.text, 1500)));
                    relevant_libs.push(r);
                }
                ArchitectResultKind::ProjectProfile => {
                    context.push_str(&format!("### Project Profile ({})\n{}\n\n", r.id, r.text));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Decision => {
                    let warning = if r.text.contains("(status: reverted)") { " ⚠ REVERTED" } else { "" };
                    context.push_str(&format!("### Architecture Decision #{}{}\n{}\n\n", r.id, warning, r.text));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Pattern => {
                    context.push_str(&format!("### Architecture Pattern #{}\n{}\n\n", r.id, r.text));
                    relevant_libs.push(r);
                }
            }
        }

        let user_message = if context.is_empty() {
            format!("Question: {}\n\nNo relevant context found in the knowledge base. Answer based on your general knowledge.", req.question)
        } else {
            format!("Question: {}\n\n## Knowledge Base Context\n{}", req.question, context)
        };

        let executor = LlmExecutor::new(&llm_client);
        let user_msgs = [llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_message)];
        let resp = executor.run(&PROMPT_ARCHITECT_ADVISE, &user_msgs)
            .await
            .map_err(|e| GitdocError::Internal(anyhow::anyhow!("LLM error: {e}")))?;

        tracing::info!(
            input_tokens = resp.input_tokens,
            output_tokens = resp.output_tokens,
            libs = relevant_libs.len(),
            rules = relevant_rules.len(),
            "architect advise completed"
        );

        Ok(Json(AdviseResponse {
            answer: resp.content,
            relevant_libs,
            relevant_rules,
        }))
    }
}
