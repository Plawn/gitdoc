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

#[derive(Deserialize)]
pub struct ConverseRequest {
    /// Natural language question
    pub q: String,
    /// Existing conversation ID (omit to create a new conversation)
    pub conversation_id: Option<i64>,
    /// Max semantic search hits (default: 8)
    pub limit: Option<usize>,
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
                        code_context.push_str(&format!("Signature: {}\n", sym.signature));
                        if let Some(ref doc) = sym.doc_comment {
                            let first_lines: String =
                                doc.lines().take(3).collect::<Vec<_>>().join("\n");
                            code_context.push_str(&format!("Doc: {}\n", first_lines));
                        }

                        // Enrich with methods for types
                        if matches!(
                            sym.kind.as_str(),
                            "struct" | "enum" | "trait" | "class" | "interface"
                        ) {
                            let children =
                                state.db.list_symbol_children(sym.id).await.unwrap_or_default();
                            let methods: Vec<_> = children
                                .iter()
                                .filter(|c| c.kind == "function")
                                .take(8)
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
    let (user_message, budget) =
        build_conversation_user_message_with_budget(&cheatsheet_content, &conversation.condensed_context, &recent_turns, &code_context, &req.q, state.config.max_prompt_tokens);

    let prompt_len = user_message.len();
    tracing::debug!(prompt_len, prompt = %user_message, "assembled LLM prompt");
    tracing::info!(
        max_tokens = budget.max_tokens,
        available = budget.available,
        question_tok = budget.question_tokens,
        cheatsheet_tok = budget.cheatsheet_tokens,
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

pub async fn delete_conversation_handler(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, conversation_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, GitdocError> {
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

    Ok(Json(serde_json::json!({ "deleted": true })))
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

async fn update_cheatsheet_from_conversation(
    db: &crate::db::Database,
    llm_client: &llm_ai::OpenAiCompatibleClient,
    snapshot_id: i64,
    turns: &[crate::db::ConversationTurnRow],
) -> anyhow::Result<()> {
    // Get repo_id from snapshot
    let snapshot = db.get_snapshot(snapshot_id).await?
        .ok_or_else(|| anyhow::anyhow!("snapshot {snapshot_id} not found"))?;
    let repo_id = &snapshot.repo_id;

    // Only update if a cheatsheet already exists (don't auto-generate initial)
    let existing = db.get_cheatsheet(repo_id).await?;
    if existing.is_none() {
        return Ok(());
    }

    // Build history string from turns
    let mut history = String::new();
    for turn in turns {
        history.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    // Extract learnings directly (conversation already deleted, use turns we saved)
    use llm_ai::{CompletionMessage, ResponseFormat, Role};
    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are an expert at extracting technical knowledge. Given a conversation about a \
             codebase, extract the key technical insights as bullet points. Focus on:\n\
             - Architectural patterns discovered\n\
             - Important types/functions and their roles\n\
             - Non-obvious behaviors or gotchas\n\
             - Conventions and patterns\n\
             - Corrections to initial assumptions\n\n\
             Only include facts that were confirmed in the conversation. \
             If the conversation was superficial with no real insights, respond with 'No significant learnings.'",
        ),
        CompletionMessage::new(Role::User, &history),
    ];

    let resp = llm_client
        .complete(&messages, Some(0.2), ResponseFormat::Text, Some(1500))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

    tracing::info!(
        repo_id,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        "conversation learnings extracted"
    );

    let learnings = resp.content.trim();
    if learnings.is_empty() || learnings == "No significant learnings." {
        tracing::debug!(repo_id, "no significant learnings from conversation");
        return Ok(());
    }

    // Update cheatsheet with learnings
    let cs = existing.unwrap();
    let (new_content, change_summary) =
        crate::cheatsheet::update_cheatsheet_from_learnings(llm_client, &cs.content, learnings).await?;

    let model_name = llm_client.name();
    db.upsert_cheatsheet(repo_id, &new_content, Some(snapshot_id), &change_summary, "conversation_reset", model_name)
        .await?;

    tracing::info!(repo_id, "cheatsheet updated from conversation learnings");
    Ok(())
}

const CONVERSE_SYSTEM_PROMPT: &str = "You are a code intelligence assistant embedded in a codebase exploration tool. \
    You answer questions about a codebase using the provided code context (symbols, docs, signatures). \
    Be precise and reference specific types, functions, and modules. \
    If the context is insufficient, say so. \
    Keep answers concise but thorough.";

/// Rough token estimate: ~4 chars per token.
fn estimate_str_tokens(s: &str) -> usize {
    s.len() / 4
}

pub struct PromptBudgetBreakdown {
    pub max_tokens: usize,
    pub available: usize,
    pub question_tokens: usize,
    pub cheatsheet_tokens: usize,
    pub code_context_tokens: usize,
    pub recent_turns_tokens: usize,
    pub condensed_tokens: usize,
    pub total_used: usize,
}

/// Build the user message with a token budget.
///
/// Allocation priority (highest first):
/// 1. Question — always full
/// 2. Cheatsheet — always full
/// 3. Code context — truncate entries from the end if over budget
/// 4. Recent turns — drop oldest first if over budget
/// 5. Condensed context — hard-truncate to remaining budget
fn build_conversation_user_message_with_budget(
    cheatsheet: &str,
    condensed_context: &str,
    recent_turns: &[crate::db::ConversationTurnRow],
    code_context: &str,
    question: &str,
    max_tokens: usize,
) -> (String, PromptBudgetBreakdown) {
    const SYSTEM_RESERVE: usize = 200;
    const ANSWER_RESERVE: usize = 3000;

    let available = max_tokens.saturating_sub(SYSTEM_RESERVE + ANSWER_RESERVE);

    // Priority 1 & 2: question and cheatsheet are always included
    let question_section = format!("## Current question\n{}", question);
    let cheatsheet_section = if !cheatsheet.is_empty() {
        format!("## Repo cheatsheet\n{}\n\n", cheatsheet)
    } else {
        String::new()
    };

    let fixed_tokens = estimate_str_tokens(&question_section) + estimate_str_tokens(&cheatsheet_section);
    let mut remaining = available.saturating_sub(fixed_tokens);

    // Priority 3: code context — split by entries (### boundaries), drop from end
    let code_section = if !code_context.is_empty() {
        let header = "## Relevant code context\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_code = remaining - header_tokens;
            let entries: Vec<&str> = code_context.split("\n### ").collect();
            let mut kept = String::new();
            let mut used = 0;
            for (i, entry) in entries.iter().enumerate() {
                let full_entry = if i == 0 { entry.to_string() } else { format!("### {}", entry) };
                let entry_tokens = estimate_str_tokens(&full_entry);
                if used + entry_tokens > budget_for_code {
                    break;
                }
                kept.push_str(&full_entry);
                if !full_entry.ends_with('\n') {
                    kept.push('\n');
                }
                used += entry_tokens;
            }
            if !kept.is_empty() {
                remaining = remaining.saturating_sub(header_tokens + used);
                format!("{}{}\n", header, kept.trim_end())
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Priority 4: recent turns — drop oldest first
    let turns_section = if !recent_turns.is_empty() {
        let header = "## Recent conversation\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_turns = remaining - header_tokens;
            // Build turn strings from newest to oldest, then reverse the kept ones
            let mut turn_strings: Vec<String> = Vec::new();
            let mut used = 0;
            for turn in recent_turns.iter().rev() {
                let s = format!("**Q:** {}\n**A:** {}\n\n", turn.question, turn.answer);
                let t = estimate_str_tokens(&s);
                if used + t > budget_for_turns {
                    break;
                }
                turn_strings.push(s);
                used += t;
            }
            turn_strings.reverse();
            if !turn_strings.is_empty() {
                remaining = remaining.saturating_sub(header_tokens + used);
                let mut section = header.to_string();
                for s in &turn_strings {
                    section.push_str(s);
                }
                section
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Priority 5: condensed context — hard-truncate to remaining budget
    let condensed_section = if !condensed_context.is_empty() && remaining > 0 {
        let header = "## Previous conversation summary\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_condensed = remaining - header_tokens;
            let max_chars = budget_for_condensed * 4; // reverse of token estimate
            let truncated = if condensed_context.len() > max_chars {
                &condensed_context[..max_chars]
            } else {
                condensed_context
            };
            format!("{}{}\n\n", header, truncated)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Assemble in reading order
    let mut user_content = String::new();
    user_content.push_str(&cheatsheet_section);
    user_content.push_str(&condensed_section);
    user_content.push_str(&turns_section);
    user_content.push_str(&code_section);
    user_content.push_str(&question_section);

    let question_tokens = estimate_str_tokens(&question_section);
    let cheatsheet_tokens = estimate_str_tokens(&cheatsheet_section);
    let code_context_tokens = estimate_str_tokens(&code_section);
    let recent_turns_tokens = estimate_str_tokens(&turns_section);
    let condensed_tokens = estimate_str_tokens(&condensed_section);
    let total_used = question_tokens + cheatsheet_tokens + code_context_tokens + recent_turns_tokens + condensed_tokens;

    let breakdown = PromptBudgetBreakdown {
        max_tokens,
        available,
        question_tokens,
        cheatsheet_tokens,
        code_context_tokens,
        recent_turns_tokens,
        condensed_tokens,
        total_used,
    };

    (user_content, breakdown)
}

fn estimate_tokens(question: &str, answer: &str) -> i32 {
    estimate_str_tokens(question) as i32 + estimate_str_tokens(answer) as i32
}

async fn condense_history(
    db: &crate::db::Database,
    llm: &llm_ai::OpenAiCompatibleClient,
    conversation_id: i64,
    condensed_up_to: i32,
) -> anyhow::Result<()> {
    use llm_ai::{CompletionMessage, ResponseFormat, Role};

    // Load only un-condensed turns (those after the current boundary)
    let turns = db.list_recent_turns(conversation_id, condensed_up_to, 100).await?;
    if turns.is_empty() {
        return Ok(());
    }

    let max_turn_index = turns.iter().map(|t| t.turn_index).max().unwrap_or(condensed_up_to);

    // Load existing condensed context to merge with
    // We re-fetch the conversation to get the latest condensed_context
    let existing_condensed = {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT condensed_context FROM conversations WHERE id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&db.pool)
        .await?;
        row.map(|r| r.0).unwrap_or_default()
    };

    // Build the input for the LLM: existing summary + new turns
    let mut input = String::new();
    if !existing_condensed.is_empty() {
        input.push_str("## Existing summary of earlier conversation\n");
        input.push_str(&existing_condensed);
        input.push_str("\n\n## New turns to incorporate\n");
    }
    for turn in &turns {
        input.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    let system_prompt = if existing_condensed.is_empty() {
        "You are a summarization assistant. Condense the following conversation about a codebase \
         into a concise summary (~300 words). Preserve key technical facts, decisions, and \
         conclusions. The summary will be used as context for future questions in this conversation."
    } else {
        "You are a summarization assistant. You are given an existing summary of earlier conversation \
         and new turns that followed. Produce a single merged summary (~300 words) that incorporates \
         all key technical facts, decisions, and conclusions from both the existing summary and the \
         new turns. The merged summary replaces the old one entirely."
    };

    let messages = vec![
        CompletionMessage::new(Role::System, system_prompt),
        CompletionMessage::new(Role::User, &input),
    ];

    let condense_input_len = input.len();
    tracing::debug!(condense_input_len, input = %input, conversation_id, "condense_history LLM prompt");

    let resp = llm
        .complete(&messages, Some(0.2), ResponseFormat::Text, Some(1000))
        .await
        .map_err(|e| anyhow::anyhow!("condensation LLM error: {e}"))?;

    tracing::debug!(condense_output_len = resp.content.len(), output = %resp.content, conversation_id, "condense_history LLM response");

    db.update_condensed_context(conversation_id, &resp.content, max_turn_index).await?;
    tracing::info!(
        conversation_id,
        condensed_up_to = max_turn_index,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        turns_condensed = turns.len(),
        "conversation history condensed"
    );
    Ok(())
}
