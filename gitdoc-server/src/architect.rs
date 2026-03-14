use anyhow::Result;
use llm_ai::{CompletionMessage, OpenAiCompatibleClient, Role};

use crate::db::Database;
use crate::embeddings::{self, EmbeddingProvider};
use crate::llm_executor::{LlmExecutor, PROMPT_ARCHITECT_COMPARE, PROMPT_ARCHITECT_PROFILE};

/// Generate a lib profile from an already-indexed repo using LLM.
pub async fn generate_lib_profile(
    llm: &OpenAiCompatibleClient,
    embedder: Option<&dyn EmbeddingProvider>,
    db: &Database,
    lib_id: &str,
    lib_name: &str,
    repo_id: &str,
    snapshot_id: i64,
    category: &str,
    version_hint: &str,
) -> Result<crate::db::LibProfileRow> {
    // Gather context from the indexed repo
    let cheatsheet_text = db
        .get_cheatsheet(repo_id)
        .await
        .ok()
        .flatten()
        .map(|cs| cs.content)
        .unwrap_or_default();

    let symbols = db.get_public_api_symbols(snapshot_id, None, 100, 0).await?;
    let sig_lines: Vec<String> = symbols
        .iter()
        .filter(|s| s.kind != "impl")
        .take(100)
        .map(|s| format!("[{}] {}: {}", s.kind, s.name, s.signature))
        .collect();

    let file_infos = db.get_snapshot_file_paths(snapshot_id).await?;
    let module_lines: Vec<String> = file_infos
        .iter()
        .filter(|f| f.file_type != "other")
        .map(|f| {
            let mod_path = crate::util::path_to_module(&f.file_path);
            format!("  {} ({} public items)", mod_path, f.public_symbol_count)
        })
        .collect();

    let mut context = String::new();
    if !cheatsheet_text.is_empty() {
        context.push_str("## Repo Cheatsheet\n");
        context.push_str(&cheatsheet_text);
        context.push_str("\n\n");
    }
    if !module_lines.is_empty() {
        context.push_str("## Module Tree\n");
        context.push_str(&module_lines.join("\n"));
        context.push_str("\n\n");
    }
    if !sig_lines.is_empty() {
        context.push_str("## Public API Symbols (top 100)\n");
        context.push_str(&sig_lines.join("\n"));
        context.push('\n');
    }

    let user_message = format!(
        "Generate a profile for the library \"{lib_name}\" (version hint: {version_hint}, category: {category}).\n\n{context}"
    );

    let executor = LlmExecutor::new(llm);
    let user = [CompletionMessage::new(Role::User, &user_message)];
    let resp = executor
        .run_anyhow(&PROMPT_ARCHITECT_PROFILE, &user)
        .await?;

    let profile_text = resp.content;
    let model_name = llm.name().to_string();

    tracing::info!(
        lib_id,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        "lib profile generated"
    );

    // Generate embedding if provider available
    let embedding = if let Some(emb) = embedder {
        let vec = emb.embed_query(&profile_text).await?;
        Some(embeddings::to_pgvector(&vec))
    } else {
        None
    };

    db.upsert_lib_profile(
        lib_id,
        lib_name,
        Some(repo_id),
        category,
        version_hint,
        &profile_text,
        "auto",
        &model_name,
        embedding,
    )
    .await?;

    db.get_lib_profile(lib_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("lib profile vanished after creation"))
}

/// Get relevant architect context for auto-injection into converse.
pub async fn get_relevant_architect_context(
    db: &Database,
    embedder: &dyn EmbeddingProvider,
    question: &str,
    threshold: f64,
    limit: i64,
) -> Result<Option<String>> {
    let query_vec = embedder.embed_query(question).await?;
    let query_pgvec = embeddings::to_pgvector(&query_vec);

    let results = db.search_architect_by_vector(&query_pgvec, limit).await?;

    let relevant: Vec<_> = results.into_iter().filter(|r| r.score > threshold).collect();

    if relevant.is_empty() {
        return Ok(None);
    }

    use crate::db::ArchitectResultKind;

    let mut output = String::from("## Technology guidance (from Architect)\n\n");
    for r in &relevant {
        let label = match r.kind {
            ArchitectResultKind::LibProfile => format!("Library profile ({})", r.id),
            ArchitectResultKind::StackRule => format!("Stack rule #{}", r.id),
            ArchitectResultKind::Cheatsheet => format!("Repo cheatsheet ({})", r.id),
            ArchitectResultKind::ProjectProfile => format!("Project profile ({})", r.id),
            ArchitectResultKind::Decision => {
                if r.text.contains("(status: reverted)") {
                    format!("⚠ Reverted decision #{}", r.id)
                } else {
                    format!("Architecture decision #{}", r.id)
                }
            }
            ArchitectResultKind::Pattern => format!("Architecture pattern #{}", r.id),
        };
        // Truncate long texts for injection
        let text = if r.text.len() > 1500 {
            format!("{}...", &r.text[..1500])
        } else {
            r.text.clone()
        };
        output.push_str(&format!("### {label}\n{text}\n\n"));
    }

    Ok(Some(output))
}

/// Compare libraries side-by-side using LLM.
pub async fn compare_libs(
    db: &Database,
    llm: &OpenAiCompatibleClient,
    lib_ids: &[String],
    criteria: &str,
) -> Result<String> {
    let mut profiles_context = String::new();

    for lib_id in lib_ids {
        match db.get_lib_profile(lib_id).await? {
            Some(profile) => {
                profiles_context.push_str(&format!(
                    "## {} ({})\nCategory: {}\nVersion: {}\n\n{}\n\n---\n\n",
                    profile.name,
                    profile.id,
                    profile.category,
                    profile.version_hint,
                    profile.profile
                ));
            }
            None => {
                profiles_context.push_str(&format!(
                    "## {lib_id}\n(No profile found in knowledge base)\n\n---\n\n"
                ));
            }
        }
    }

    let user_message = format!(
        "Compare these libraries for the following criteria: {criteria}\n\n{profiles_context}"
    );

    let executor = LlmExecutor::new(llm);
    let user = [CompletionMessage::new(Role::User, &user_message)];
    let resp = executor
        .run_anyhow(&PROMPT_ARCHITECT_COMPARE, &user)
        .await?;

    tracing::info!(
        libs = ?lib_ids,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        "lib comparison completed"
    );

    Ok(resp.content)
}
