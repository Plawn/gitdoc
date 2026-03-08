use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;
use super::prompt_budget::{estimate_str_tokens, estimate_tokens, build_conversation_user_message_with_budget};
use super::condensation::{condense_history, update_cheatsheet_from_conversation};

#[derive(Deserialize)]
pub struct ConverseRequest {
    /// Natural language question
    pub q: String,
    /// Existing conversation ID (omit to create a new conversation)
    pub conversation_id: Option<i64>,
    /// Max semantic search hits (default: 8)
    pub limit: Option<usize>,
    /// Detail level: "brief", "detailed", or "with_source" (default: "detailed")
    pub detail_level: Option<String>,
}

#[derive(Serialize)]
pub struct ConverseResponse {
    pub conversation_id: i64,
    pub answer: String,
    pub sources: Vec<SourceRef>,
    pub turn_index: i32,
}

#[derive(Serialize, Clone)]
pub struct SourceRef {
    pub kind: String,
    pub name: String,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<i64>,
}

const CONVERSE_SYSTEM_PROMPT: &str = "You are a code intelligence assistant embedded in a codebase exploration tool. \
    You answer questions about a codebase using the provided code context (symbols, docs, signatures). \
    Be precise and reference specific types, functions, and modules. \
    When showing code from the provided context, always cite the source file path (e.g. `src/foo.rs`). \
    If you generate example code that is NOT from the context, mark it clearly as `[generated example]`. \
    Prefer quoting verbatim from the provided context over paraphrasing or rewriting code. \
    If you cannot provide the exact source code for a symbol the user is asking about, \
    append: \"Tip: use `set_mode(\\\"granular\\\")` then `get_symbol` for the exact source code.\" \
    If the context is insufficient, say so. \
    Keep answers concise but thorough.";

pub async fn converse(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Json(req): Json<ConverseRequest>,
) -> Result<Json<ConverseResponse>, GitdocError> {
    if req.q.is_empty() {
        return Err(GitdocError::BadRequest("q must be non-empty".into()));
    }

    let llm_client = state.llm_client.as_ref().ok_or_else(|| {
        GitdocError::ServiceUnavailable("no LLM provider configured — converse requires LLM".into())
    })?;

    // Step 0: Load cheatsheet for this repo (via snapshot → repo_id)
    let cheatsheet_content = {
        let snapshot = state.db.get_snapshot(snapshot_id).await?
            .ok_or_else(|| GitdocError::NotFound(format!("snapshot {snapshot_id} not found")))?;
        state.db.get_cheatsheet(&snapshot.repo_id).await
            .ok()
            .flatten()
            .map(|cs| cs.content)
            .unwrap_or_default()
    };

    // Step 0b: Load architect context if auto mode
    let architect_context = if state.config.architect_mode == crate::config::ArchitectMode::Auto {
        if let Some(ref embedder) = state.embedder {
            crate::architect::get_relevant_architect_context(
                &state.db,
                embedder.as_ref(),
                &req.q,
                0.3,
                3,
            )
            .await
            .unwrap_or(None)
        } else {
            None
        }
    } else {
        None
    };

    // Step 1: Resolve or create conversation
    let conversation_id = if let Some(cid) = req.conversation_id {
        let conv = state.db.get_conversation(cid, snapshot_id).await?;
        if conv.is_none() {
            return Err(GitdocError::NotFound(format!(
                "conversation {cid} not found for snapshot {snapshot_id}"
            )));
        }
        cid
    } else {
        state.db.create_conversation(snapshot_id).await?
    };

    let limit = req.limit.unwrap_or(8);
    let detail_level = req.detail_level.as_deref().unwrap_or("detailed");

    // Step 2: Semantic search on the question
    let mut sources: Vec<SourceRef> = Vec::new();
    let mut code_context = String::new();

    if let Some(ref embedder) = state.embedder {
        let search_start = std::time::Instant::now();
        let query_vec = embedder
            .embed_query(&req.q)
            .await
            .map_err(|e| GitdocError::Internal(e))?;
        let file_ids = state.db.get_file_ids_for_snapshot(snapshot_id).await?;
        let query_pgvec = embeddings::to_pgvector(&query_vec);

        let search_results = state
            .db
            .search_embeddings_by_vector(&query_pgvec, &file_ids, "all", limit as i64)
            .await?;
        let search_ms = search_start.elapsed().as_millis() as u64;
        let search_hits = search_results.len();
        tracing::info!(search_ms, search_hits, "semantic search completed");

        let docs = state.db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_default();
        let mut seen_symbols: HashSet<i64> = HashSet::new();

        for r in &search_results {
            match r.source_type.as_str() {
                "symbol" => {
                    if seen_symbols.contains(&r.source_id) {
                        continue;
                    }
                    seen_symbols.insert(r.source_id);

                    if let Ok(Some(sym)) = state.db.get_symbol_by_id(r.source_id).await {
                        code_context.push_str(&format!(
                            "### {} ({}) — {}\n",
                            sym.name, sym.kind, sym.file_path
                        ));
                        code_context.push_str(&format!("[source: {}, lines {}-{}]\n", sym.file_path, sym.line_start, sym.line_end));
                        code_context.push_str(&format!("Signature: {}\n", sym.signature));

                        if detail_level == "with_source" {
                            // Include full source body
                            if !sym.body.is_empty() {
                                code_context.push_str(&format!("```\n{}\n```\n", sym.body));
                            }
                            if let Some(ref doc) = sym.doc_comment {
                                code_context.push_str(&format!("Doc: {}\n", doc));
                            }
                        } else {
                            if let Some(ref doc) = sym.doc_comment {
                                let doc_lines = if detail_level == "brief" { 1 } else { 3 };
                                let first_lines: String =
                                    doc.lines().take(doc_lines).collect::<Vec<_>>().join("\n");
                                code_context.push_str(&format!("Doc: {}\n", first_lines));
                            }
                        }

                        // Enrich with methods for types
                        if matches!(
                            sym.kind.as_str(),
                            "struct" | "enum" | "trait" | "class" | "interface"
                        ) {
                            let children =
                                state.db.list_symbol_children(sym.id).await.unwrap_or_default();
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

        let symbol_count = sources.iter().filter(|s| s.kind == "symbol").count();
        let doc_count = sources.iter().filter(|s| s.kind == "doc").count();
        let code_context_tokens = estimate_str_tokens(&code_context);
        tracing::info!(symbol_count, doc_count, code_context_tokens, "source enrichment done");
    }

    // Step 3: Load conversation history
    let conversation = state.db.get_conversation(conversation_id, snapshot_id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("conversation vanished")))?;

    let recent_turns = state.db.list_recent_turns(conversation_id, conversation.condensed_up_to, 10).await?;

    let recent_turn_count = recent_turns.len();
    let condensed_tokens = estimate_str_tokens(&conversation.condensed_context);
    let has_condensed = !conversation.condensed_context.is_empty();
    tracing::info!(conversation_id, recent_turn_count, has_condensed, condensed_tokens, condensed_up_to = conversation.condensed_up_to, "conversation context loaded");

    // Step 4: Build prompt and call LLM
    let architect_ctx_str = architect_context.as_deref().unwrap_or("");
    let (user_message, budget) =
        build_conversation_user_message_with_budget(&cheatsheet_content, architect_ctx_str, &conversation.condensed_context, &recent_turns, &code_context, &req.q, state.config.max_prompt_tokens);

    let prompt_len = user_message.len();
    tracing::debug!(prompt_len, prompt = %user_message, "assembled LLM prompt");
    tracing::info!(
        max_tokens = budget.max_tokens,
        available = budget.available,
        question_tok = budget.question_tokens,
        cheatsheet_tok = budget.cheatsheet_tokens,
        architect_tok = budget.architect_tokens,
        code_ctx_tok = budget.code_context_tokens,
        turns_tok = budget.recent_turns_tokens,
        condensed_tok = budget.condensed_tokens,
        total_used = budget.total_used,
        "prompt budget allocation"
    );

    let messages = vec![
        llm_ai::CompletionMessage::new(llm_ai::Role::System, CONVERSE_SYSTEM_PROMPT),
        llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_message),
    ];

    let llm_start = std::time::Instant::now();
    let resp = llm_client
        .complete(&messages, Some(0.3), llm_ai::ResponseFormat::Text, Some(3000))
        .await
        .map_err(|e| GitdocError::Internal(anyhow::anyhow!("LLM error: {e}")))?;

    let answer = resp.content;
    let input_tokens = resp.input_tokens;
    let output_tokens = resp.output_tokens;
    let cached_tokens = resp.cached_tokens;

    tracing::debug!(answer_len = answer.len(), answer = %answer, source_count = sources.len(), "LLM response");

    // Step 5: Persist the turn
    let token_estimate = estimate_tokens(&req.q, &answer);
    let sources_json = serde_json::to_value(&sources).unwrap_or_default();
    let turn_index = state
        .db
        .append_turn(conversation_id, &req.q, &answer, &sources_json, token_estimate)
        .await?;

    let elapsed_ms = llm_start.elapsed().as_millis() as u64;
    tracing::info!(
        conversation_id,
        turn_index,
        input_tokens,
        output_tokens,
        cached_tokens,
        elapsed_ms,
        source_count = sources.len(),
        "converse turn completed"
    );

    // Step 6: Background condensation if tokens exceed threshold
    let new_raw_tokens = conversation.raw_turn_tokens + token_estimate;
    let condensation_threshold = state.config.condensation_threshold as i32;
    if new_raw_tokens > condensation_threshold {
        let db = state.db.clone();
        let llm = llm_client.clone();
        let cid = conversation_id;
        let condensed_up_to = conversation.condensed_up_to;
        tokio::spawn(async move {
            if let Err(e) = condense_history(&db, &llm, cid, condensed_up_to).await {
                tracing::warn!(conversation_id = cid, error = %e, "failed to condense conversation history");
            }
        });
    }

    Ok(Json(ConverseResponse {
        conversation_id,
        answer,
        sources,
        turn_index,
    }))
}

#[derive(Serialize)]
pub struct DeletedResponse {
    pub deleted: bool,
}

pub async fn delete_conversation_handler(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, conversation_id)): Path<(i64, i64)>,
) -> Result<Json<DeletedResponse>, GitdocError> {
    // Extract learnings BEFORE deleting (needs conversation turns)
    let should_update_cheatsheet = state.llm_client.is_some();
    let turns_for_learnings = if should_update_cheatsheet {
        state.db.list_recent_turns(conversation_id, -1, 100).await.ok()
    } else {
        None
    };

    let deleted = state.db.delete_conversation(conversation_id, snapshot_id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!(
            "conversation {conversation_id} not found for snapshot {snapshot_id}"
        )));
    }

    // Background: extract learnings and update cheatsheet if one exists
    if let (Some(turns), Some(llm_client)) = (turns_for_learnings, state.llm_client.clone()) {
        if !turns.is_empty() {
            let db = state.db.clone();
            tokio::spawn(async move {
                if let Err(e) = update_cheatsheet_from_conversation(&db, &llm_client, snapshot_id, &turns).await {
                    tracing::warn!(snapshot_id, error = %e, "failed to update cheatsheet from conversation");
                }
            });
        }
    }

    Ok(Json(DeletedResponse { deleted: true }))
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

pub async fn list_conversations_handler(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<crate::db::ConversationRow>>, GitdocError> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    let (items, total) = state.db.list_conversations(snapshot_id, limit, offset).await?;

    Ok(Json(PaginatedResponse { items, total, limit, offset }))
}

pub async fn list_turns_handler(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, conversation_id)): Path<(i64, i64)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<crate::db::ConversationTurnRow>>, GitdocError> {
    // Validate conversation belongs to snapshot
    let conv = state.db.get_conversation(conversation_id, snapshot_id).await?;
    if conv.is_none() {
        return Err(GitdocError::NotFound(format!(
            "conversation {conversation_id} not found for snapshot {snapshot_id}"
        )));
    }

    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0).max(0);

    let (items, total) = state.db.list_all_turns(conversation_id, limit, offset).await?;

    Ok(Json(PaginatedResponse { items, total, limit, offset }))
}
