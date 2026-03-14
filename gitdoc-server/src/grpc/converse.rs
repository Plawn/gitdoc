use r2e::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use gitdoc_api_types::responses::SourceRef;

use super::proto;
use crate::AppState;
use crate::embeddings;
use crate::llm_executor::{LlmExecutor, PROMPT_CONVERSE};

#[derive(Controller)]
#[controller(state = AppState)]
pub struct ConverseGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
    #[inject]
    config: Arc<crate::config::Config>,
}

#[grpc_routes(proto::converse_service_server::ConverseService)]
impl ConverseGrpcService {
    async fn converse(
        &self,
        request: Request<proto::ConverseRequest>,
    ) -> Result<Response<proto::ConverseResponse>, Status> {
        let req = request.into_inner();
        if req.q.is_empty() {
            return Err(Status::invalid_argument("q must be non-empty"));
        }

        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;
        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| Status::unavailable("no embedding provider configured"))?;

        // Step 0: Load cheatsheet for this repo
        let cheatsheet_content = {
            let snapshot = self
                .db
                .get_snapshot(req.snapshot_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::not_found(format!("snapshot {} not found", req.snapshot_id)))?;
            match self.db.get_cheatsheet(&snapshot.repo_id).await {
                Ok(cs) => cs.map(|c| c.content).unwrap_or_default(),
                Err(e) => {
                    tracing::warn!(repo_id = %snapshot.repo_id, error = %e, "failed to load cheatsheet");
                    String::new()
                }
            }
        };

        // Step 0b: Load architect context if auto mode
        let architect_context = if self.config.architect_mode == crate::config::ArchitectMode::Auto {
            crate::architect::get_relevant_architect_context(
                &self.db,
                embedder.as_ref(),
                &req.q,
                0.3,
                3,
            )
            .await
            .unwrap_or(None)
        } else {
            None
        };

        // Step 1: Resolve or create conversation
        let conversation_id = if req.conversation_id != 0 {
            let conv = self
                .db
                .get_conversation(req.conversation_id, req.snapshot_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            if conv.is_none() {
                return Err(Status::not_found(format!(
                    "conversation {} not found for snapshot {}",
                    req.conversation_id, req.snapshot_id
                )));
            }
            req.conversation_id
        } else {
            self.db
                .create_conversation(req.snapshot_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
        };

        let limit = if req.limit == 0 { 8 } else { req.limit as usize };
        let detail_level = if req.detail_level.is_empty() {
            "detailed"
        } else {
            &req.detail_level
        };

        // Step 2: Semantic search on the question
        let mut sources: Vec<SourceRef> = Vec::new();
        let mut code_context = String::new();

        {
            let query_vec = embedder
                .embed_query(&req.q)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let file_ids = self
                .db
                .get_file_ids_for_snapshot(req.snapshot_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let query_pgvec = embeddings::to_pgvector(&query_vec);

            let search_results = self
                .db
                .search_embeddings_by_vector(&query_pgvec, &file_ids, "all", limit as i64)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let docs = self
                .db
                .list_docs_for_snapshot(req.snapshot_id)
                .await
                .unwrap_or_default();
            let mut seen_symbols: HashSet<i64> = HashSet::new();

            for r in &search_results {
                match r.source_type.as_str() {
                    "symbol" => {
                        if seen_symbols.contains(&r.source_id) {
                            continue;
                        }
                        seen_symbols.insert(r.source_id);

                        if let Ok(Some(sym)) = self.db.get_symbol_by_id(r.source_id).await {
                            code_context.push_str(&format!(
                                "### {} ({}) — {}\n",
                                sym.name, sym.kind, sym.file_path
                            ));
                            code_context.push_str(&format!(
                                "[source: {}, lines {}-{}]\n",
                                sym.file_path, sym.line_start, sym.line_end
                            ));
                            code_context.push_str(&format!("Signature: {}\n", sym.signature));

                            if detail_level == "with_source" {
                                if !sym.body.is_empty() {
                                    code_context
                                        .push_str(&format!("```\n{}\n```\n", sym.body));
                                }
                                if let Some(ref doc) = sym.doc_comment {
                                    code_context.push_str(&format!("Doc: {}\n", doc));
                                }
                            } else {
                                if let Some(ref doc) = sym.doc_comment {
                                    let doc_lines =
                                        if detail_level == "brief" { 1 } else { 3 };
                                    let first_lines: String = doc
                                        .lines()
                                        .take(doc_lines)
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    code_context.push_str(&format!("Doc: {}\n", first_lines));
                                }
                            }

                            if matches!(
                                sym.kind.as_str(),
                                "struct" | "enum" | "trait" | "class" | "interface"
                            ) {
                                let children = self
                                    .db
                                    .list_symbol_children(sym.id)
                                    .await
                                    .unwrap_or_default();
                                let method_limit = match detail_level {
                                    "brief" => 4,
                                    "with_source" => 16,
                                    _ => 8,
                                };
                                let methods: Vec<_> = children
                                    .iter()
                                    .filter(|c| c.kind == "function")
                                    .take(method_limit)
                                    .collect();
                                if !methods.is_empty() {
                                    code_context.push_str("Methods:\n");
                                    for m in &methods {
                                        code_context.push_str(&format!(
                                            "  - {}: {}\n",
                                            m.name, m.signature
                                        ));
                                    }
                                }
                            }

                            code_context.push('\n');
                            sources.push(SourceRef {
                                kind: "symbol".into(),
                                name: sym.qualified_name,
                                file_path: sym.file_path,
                                symbol_id: Some(sym.id),
                            });
                        }
                    }
                    "doc_chunk" => {
                        if let Some(doc) = docs.iter().find(|d| d.id == r.source_id) {
                            code_context.push_str(&format!(
                                "### Doc: {} ({})\n{}\n\n",
                                doc.title.as_deref().unwrap_or("untitled"),
                                doc.file_path,
                                r.text
                            ));
                            sources.push(SourceRef {
                                kind: "doc".into(),
                                name: doc.title.clone().unwrap_or_default(),
                                file_path: doc.file_path.clone(),
                                symbol_id: None,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // Step 3: Load conversation history
        let conversation = self
            .db
            .get_conversation(conversation_id, req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("conversation vanished"))?;

        let recent_turns = self
            .db
            .list_recent_turns(conversation_id, conversation.condensed_up_to, 10)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Step 4: Build prompt and call LLM
        let architect_ctx_str = architect_context.as_deref().unwrap_or("");
        let (user_message, _budget) =
            crate::api::prompt_budget::build_conversation_user_message_with_budget(
                &cheatsheet_content,
                architect_ctx_str,
                &conversation.condensed_context,
                &recent_turns,
                &code_context,
                &req.q,
                self.config.max_prompt_tokens,
            );

        let executor = LlmExecutor::new(&llm_client);
        let user_msgs = [llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_message)];
        let resp = executor.run(&PROMPT_CONVERSE, &user_msgs)
            .await
            .map_err(|e| Status::internal(format!("LLM error: {e}")))?;

        let answer = resp.content;

        // Step 5: Persist the turn
        let token_estimate =
            crate::api::prompt_budget::estimate_tokens(&req.q, &answer);
        let sources_json = serde_json::to_value(&sources).unwrap_or_default();
        let turn_index = self
            .db
            .append_turn(conversation_id, &req.q, &answer, &sources_json, token_estimate)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Step 6: Background condensation if tokens exceed threshold
        let new_raw_tokens = conversation.raw_turn_tokens + token_estimate;
        let condensation_threshold = self.config.condensation_threshold as i32;
        if new_raw_tokens > condensation_threshold {
            let db = self.db.clone();
            let llm = llm_client.clone();
            let cid = conversation_id;
            let condensed_up_to = conversation.condensed_up_to;
            tokio::spawn(async move {
                if let Err(e) =
                    crate::api::condensation::condense_history(&db, &llm, cid, condensed_up_to)
                        .await
                {
                    tracing::warn!(
                        conversation_id = cid,
                        error = %e,
                        "failed to condense conversation history"
                    );
                }
            });
        }

        Ok(Response::new(proto::ConverseResponse {
            conversation_id,
            answer,
            sources: sources.into_iter().map(Into::into).collect(),
            turn_index,
        }))
    }

    async fn reset_conversation(
        &self,
        request: Request<proto::ResetConversationRequest>,
    ) -> Result<Response<proto::ResetConversationResponse>, Status> {
        let req = request.into_inner();
        let deleted = self
            .db
            .delete_conversation(req.conversation_id, req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ResetConversationResponse { deleted }))
    }
}
