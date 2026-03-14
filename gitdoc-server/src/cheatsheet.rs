use std::sync::Arc;

use anyhow::Result;
use llm_ai::{CompletionMessage, OpenAiCompatibleClient, Role};

use crate::db::Database;
use crate::embeddings::{self, EmbeddingProvider};
use crate::llm_executor::{
    LlmExecutor, PROMPT_CHEATSHEET_GENERATE, PROMPT_CHEATSHEET_LEARNINGS, PROMPT_CHEATSHEET_UPDATE,
};

/// Generate an initial cheatsheet from a snapshot's structure.
pub async fn generate_cheatsheet(
    client: &OpenAiCompatibleClient,
    db: &Database,
    snapshot_id: i64,
) -> Result<(String, String)> {
    // Gather context
    let file_infos = db.get_snapshot_file_paths(snapshot_id).await?;
    let modules: Vec<String> = file_infos
        .iter()
        .filter(|f| f.file_type != "other")
        .map(|f| {
            let mod_path = crate::util::path_to_module(&f.file_path);
            format!("  {} ({} public items)", mod_path, f.public_symbol_count)
        })
        .collect();

    let symbols = db
        .get_public_api_symbols(snapshot_id, None, 150, 0)
        .await?;
    let sig_lines: Vec<String> = symbols
        .iter()
        .filter(|s| s.kind != "impl")
        .take(150)
        .map(|s| format!("  [{}] {}: {}", s.kind, s.name, s.signature))
        .collect();

    // Try to get README
    let readme = db
        .get_doc_content(snapshot_id, "README.md")
        .await
        .ok()
        .flatten()
        .and_then(|d| d.content)
        .unwrap_or_default();

    // Try to get crate summary if it exists
    let crate_summary = db
        .get_summary(snapshot_id, "crate")
        .await
        .ok()
        .flatten()
        .map(|s| s.content)
        .unwrap_or_default();

    let mut context = String::new();
    if !readme.is_empty() {
        // Truncate README to first 2000 chars
        let readme_excerpt = if readme.len() > 2000 {
            &readme[..2000]
        } else {
            &readme
        };
        context.push_str(&format!("## README (excerpt)\n{}\n\n", readme_excerpt));
    }
    if !crate_summary.is_empty() {
        context.push_str(&format!(
            "## Existing crate summary\n{}\n\n",
            crate_summary
        ));
    }
    context.push_str(&format!(
        "## Modules\n{}\n\n## Key public symbols (first 150)\n{}",
        modules.join("\n"),
        sig_lines.join("\n"),
    ));

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &context)];
    let resp = executor
        .run_anyhow(&PROMPT_CHEATSHEET_GENERATE, &user)
        .await?;

    parse_content_and_summary(&resp.content)
}

/// Update an existing cheatsheet with new learnings.
pub async fn update_cheatsheet_from_learnings(
    client: &OpenAiCompatibleClient,
    current_content: &str,
    learnings: &str,
) -> Result<(String, String)> {
    let context = format!(
        "## Current cheatsheet\n{}\n\n## New learnings from conversation\n{}",
        current_content, learnings
    );

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &context)];
    let resp = executor
        .run_anyhow(&PROMPT_CHEATSHEET_UPDATE, &user)
        .await?;

    parse_content_and_summary(&resp.content)
}

/// Extract technical learnings from a conversation's turns.
pub async fn extract_conversation_learnings(
    client: &OpenAiCompatibleClient,
    db: &Database,
    conversation_id: i64,
) -> Result<String> {
    let turns = db.list_recent_turns(conversation_id, -1, 100).await?;
    if turns.is_empty() {
        return Ok(String::new());
    }

    let mut history = String::new();
    for turn in &turns {
        history.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &history)];
    let resp = executor
        .run_anyhow(&PROMPT_CHEATSHEET_LEARNINGS, &user)
        .await?;

    Ok(resp.content)
}

/// Orchestrator: generate or update a cheatsheet and persist it.
/// Returns the patch_id.
pub async fn generate_and_store_cheatsheet(
    client: Arc<OpenAiCompatibleClient>,
    db: &Database,
    repo_id: &str,
    snapshot_id: Option<i64>,
    trigger: &str,
    learnings: Option<&str>,
    embedder: Option<&dyn EmbeddingProvider>,
) -> Result<i64> {
    let existing = db.get_cheatsheet(repo_id).await?;

    let (new_content, change_summary) = match (learnings, &existing) {
        // Has learnings and existing cheatsheet → update
        (Some(l), Some(cs)) if !l.is_empty() && l != "No significant learnings." => {
            update_cheatsheet_from_learnings(&client, &cs.content, l).await?
        }
        // No learnings or no existing cheatsheet → generate from scratch
        _ => {
            let sid = snapshot_id
                .ok_or_else(|| anyhow::anyhow!("snapshot_id required for initial generation"))?;
            generate_cheatsheet(&client, db, sid).await?
        }
    };

    // Generate embedding for architect search
    let content_embedding = if let Some(emb) = embedder {
        match emb.embed_query(&new_content).await {
            Ok(vec) => Some(embeddings::to_pgvector(&vec)),
            Err(e) => {
                tracing::warn!(error = %e, "failed to embed cheatsheet content");
                None
            }
        }
    } else {
        None
    };

    let model_name = client.name();
    let patch_id = db
        .upsert_cheatsheet(
            repo_id,
            &new_content,
            snapshot_id,
            &change_summary,
            trigger,
            model_name,
            content_embedding,
        )
        .await?;

    Ok(patch_id)
}

/// Parse LLM response into (content, summary) split by `---SUMMARY---` marker.
fn parse_content_and_summary(response: &str) -> Result<(String, String)> {
    if let Some(idx) = response.find("---SUMMARY---") {
        let content = response[..idx].trim().to_string();
        let summary = response[idx + "---SUMMARY---".len()..].trim().to_string();
        Ok((content, summary))
    } else {
        // No separator found — use the whole response as content
        Ok((
            response.trim().to_string(),
            "Generated cheatsheet".to_string(),
        ))
    }
}
