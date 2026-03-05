use std::sync::Arc;

use anyhow::Result;
use llm_ai::{CompletionMessage, OpenAiCompatibleClient, ResponseFormat, Role};

use crate::db::Database;
use crate::embeddings::{self, EmbeddingProvider};

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
        context.push_str(&format!("## Existing crate summary\n{}\n\n", crate_summary));
    }
    context.push_str(&format!(
        "## Modules\n{}\n\n## Key public symbols (first 150)\n{}",
        modules.join("\n"),
        sig_lines.join("\n"),
    ));

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a technical documentation expert. Given a codebase's structure, produce a \
             structured cheatsheet that an AI agent can use to quickly understand the repo. \
             Use these sections:\n\
             - **Purpose**: What the project does (1-2 sentences)\n\
             - **Architecture**: High-level organization (key modules, layers)\n\
             - **Key Types**: Most important structs/enums/traits with one-line descriptions\n\
             - **Key Functions/Entry Points**: Main functions and how to use them\n\
             - **Patterns & Conventions**: Naming conventions, error handling, common patterns\n\
             - **Gotchas**: Non-obvious behaviors, footguns, important constraints\n\n\
             Be precise, concise, and factual. Do not invent information not present in the data.\n\n\
             After the cheatsheet, output a line containing only `---SUMMARY---`, \
             then a 1-2 sentence description of what was generated (for the patch log).",
        ),
        CompletionMessage::new(Role::User, &context),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(3000))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

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

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are updating an existing repo cheatsheet with new insights learned from a \
             conversation. Merge the learnings into the existing cheatsheet:\n\
             - Add new information to the appropriate sections\n\
             - Correct any information that the learnings contradict\n\
             - Do NOT remove existing correct information\n\
             - Keep the same section structure (Purpose, Architecture, Key Types, \
               Key Functions/Entry Points, Patterns & Conventions, Gotchas)\n\
             - If learnings don't add anything new, return the cheatsheet unchanged\n\n\
             Output the full updated cheatsheet, then a line containing only `---SUMMARY---`, \
             then a 1-2 sentence description of what changed (or 'No changes' if nothing was updated).",
        ),
        CompletionMessage::new(Role::User, &context),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(3000))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

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

    let resp = client
        .complete(&messages, Some(0.2), ResponseFormat::Text, Some(1500))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

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
            let sid = snapshot_id.ok_or_else(|| anyhow::anyhow!("snapshot_id required for initial generation"))?;
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
        .upsert_cheatsheet(repo_id, &new_content, snapshot_id, &change_summary, trigger, model_name, content_embedding)
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
        Ok((response.trim().to_string(), "Generated cheatsheet".to_string()))
    }
}
