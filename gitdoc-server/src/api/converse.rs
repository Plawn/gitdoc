use axum::{
    Json,
    extract::{Path, State},
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
    }

    // Step 3: Load conversation history
    let conversation = state.db.get_conversation(conversation_id, snapshot_id).await?
        .ok_or_else(|| GitdocError::Internal(anyhow::anyhow!("conversation vanished")))?;

    let recent_turns = state.db.list_recent_turns(conversation_id, 10).await?;

    // Step 4: Build prompt and call LLM
    let user_message =
        build_conversation_user_message(&conversation.condensed_context, &recent_turns, &code_context, &req.q);

    let messages = vec![
        llm_ai::CompletionMessage::new(llm_ai::Role::System, CONVERSE_SYSTEM_PROMPT),
        llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_message),
    ];

    let resp = llm_client
        .complete(&messages, Some(0.3), llm_ai::ResponseFormat::Text, Some(3000))
        .await
        .map_err(|e| GitdocError::Internal(anyhow::anyhow!("LLM error: {e}")))?;

    let answer = resp.content;

    // Step 5: Persist the turn
    let token_estimate = estimate_tokens(&req.q, &answer);
    let sources_json = serde_json::to_value(&sources).unwrap_or_default();
    let turn_index = state
        .db
        .append_turn(conversation_id, &req.q, &answer, &sources_json, token_estimate)
        .await?;

    // Step 6: Background condensation if tokens exceed threshold
    let new_raw_tokens = conversation.raw_turn_tokens + token_estimate;
    if new_raw_tokens > 6000 {
        let db = state.db.clone();
        let llm = llm_client.clone();
        let cid = conversation_id;
        tokio::spawn(async move {
            if let Err(e) = condense_history(&db, &llm, cid).await {
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
    let deleted = state.db.delete_conversation(conversation_id, snapshot_id).await?;
    if !deleted {
        return Err(GitdocError::NotFound(format!(
            "conversation {conversation_id} not found for snapshot {snapshot_id}"
        )));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

const CONVERSE_SYSTEM_PROMPT: &str = "You are a code intelligence assistant embedded in a codebase exploration tool. \
    You answer questions about a codebase using the provided code context (symbols, docs, signatures). \
    Be precise and reference specific types, functions, and modules. \
    If the context is insufficient, say so. \
    Keep answers concise but thorough.";

fn build_conversation_user_message(
    condensed_context: &str,
    recent_turns: &[crate::db::ConversationTurnRow],
    code_context: &str,
    question: &str,
) -> String {
    let mut user_content = String::new();

    // Condensed history from previous turns
    if !condensed_context.is_empty() {
        user_content.push_str("## Previous conversation summary\n");
        user_content.push_str(condensed_context);
        user_content.push_str("\n\n");
    }

    // Recent raw turns
    if !recent_turns.is_empty() {
        user_content.push_str("## Recent conversation\n");
        for turn in recent_turns {
            user_content.push_str(&format!("**Q:** {}\n**A:** {}\n\n", turn.question, turn.answer));
        }
    }

    // Code context from semantic search
    if !code_context.is_empty() {
        user_content.push_str("## Relevant code context\n");
        user_content.push_str(code_context);
        user_content.push_str("\n");
    }

    user_content.push_str(&format!("## Current question\n{}", question));
    user_content
}

fn estimate_tokens(question: &str, answer: &str) -> i32 {
    // Rough estimate: ~4 chars per token
    ((question.len() + answer.len()) / 4) as i32
}

async fn condense_history(
    db: &crate::db::Database,
    llm: &llm_ai::OpenAiCompatibleClient,
    conversation_id: i64,
) -> anyhow::Result<()> {
    use llm_ai::{CompletionMessage, ResponseFormat, Role};

    let turns = db.list_recent_turns(conversation_id, 100).await?;
    if turns.is_empty() {
        return Ok(());
    }

    let mut history = String::new();
    for turn in &turns {
        history.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a summarization assistant. Condense the following conversation about a codebase \
             into a concise summary (~300 words). Preserve key technical facts, decisions, and \
             conclusions. The summary will be used as context for future questions in this conversation.",
        ),
        CompletionMessage::new(Role::User, &history),
    ];

    let resp = llm
        .complete(&messages, Some(0.2), ResponseFormat::Text, Some(1000))
        .await
        .map_err(|e| anyhow::anyhow!("condensation LLM error: {e}"))?;

    db.update_condensed_context(conversation_id, &resp.content).await?;
    tracing::info!(conversation_id, "conversation history condensed");
    Ok(())
}
