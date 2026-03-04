use std::sync::Arc;

use anyhow::Result;
use llm_ai::{CompletionMessage, OpenAiCompatibleClient, ResponseFormat, Role};

use crate::db::Database;

/// Generate a crate-level summary from the module tree and top-level symbols.
pub async fn generate_crate_summary(
    client: &OpenAiCompatibleClient,
    db: &Database,
    snapshot_id: i64,
) -> Result<String> {
    // Gather context: module tree overview + top public symbols
    let file_infos = db.get_snapshot_file_paths(snapshot_id).await?;
    let modules: Vec<String> = file_infos
        .iter()
        .filter(|f| f.file_type != "other")
        .map(|f| {
            let mod_path = crate::api::snapshots::path_to_module(&f.file_path);
            format!("  {} ({} public items)", mod_path, f.public_symbol_count)
        })
        .collect();

    let symbols = db
        .get_public_api_symbols(snapshot_id, None, 200, 0)
        .await?;
    let sig_lines: Vec<String> = symbols
        .iter()
        .filter(|s| s.kind != "impl")
        .take(100)
        .map(|s| format!("  [{}] {}: {}", s.kind, s.name, s.signature))
        .collect();

    let context = format!(
        "## Modules\n{}\n\n## Key public symbols (first 100)\n{}",
        modules.join("\n"),
        sig_lines.join("\n"),
    );

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a technical documentation expert. Given the module structure and public API \
             of a Rust crate, produce a concise summary (3-8 paragraphs) that explains:\n\
             1. What the crate does (purpose)\n\
             2. How it's organized (key modules)\n\
             3. Main types and their roles\n\
             4. Typical usage patterns\n\
             Be precise and technical. Do not invent information not present in the data.",
        ),
        CompletionMessage::new(Role::User, &context),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(2000))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

    Ok(resp.content)
}

/// Generate a module-level summary from the module's public symbols.
pub async fn generate_module_summary(
    client: &OpenAiCompatibleClient,
    db: &Database,
    snapshot_id: i64,
    module_path: &str,
) -> Result<String> {
    let symbols = db
        .get_public_api_symbols(snapshot_id, Some(module_path), 500, 0)
        .await?;

    if symbols.is_empty() {
        return Ok(format!("Module `{}` has no public symbols.", module_path));
    }

    let sig_lines: Vec<String> = symbols
        .iter()
        .filter(|s| s.kind != "impl")
        .map(|s| {
            let doc_hint = s
                .doc_comment
                .as_deref()
                .and_then(|d| d.lines().next())
                .unwrap_or("");
            format!("  [{}] {}: {} // {}", s.kind, s.name, s.signature, doc_hint)
        })
        .collect();

    let context = format!(
        "Module: {}\n\nPublic symbols:\n{}",
        module_path,
        sig_lines.join("\n"),
    );

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a technical documentation expert. Given the public symbols of a Rust module, \
             produce a concise summary (2-4 paragraphs) that explains what this module provides, \
             its key types and functions, and how they relate to each other. Be precise.",
        ),
        CompletionMessage::new(Role::User, &context),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(1500))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

    Ok(resp.content)
}

/// Generate a type-level summary from the symbol detail and its context.
pub async fn generate_type_summary(
    client: &OpenAiCompatibleClient,
    db: &Database,
    snapshot_id: i64,
    symbol_id: i64,
) -> Result<String> {
    let symbol = db
        .get_symbol_by_id(symbol_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("symbol not found"))?;

    let children = db.list_symbol_children(symbol_id).await.unwrap_or_default();
    let impls = db
        .get_implementations(symbol_id, snapshot_id)
        .await
        .unwrap_or_default();

    let methods: Vec<String> = children
        .iter()
        .filter(|c| c.kind == "function")
        .map(|c| {
            let doc_hint = c
                .doc_comment
                .as_deref()
                .and_then(|d| d.lines().next())
                .unwrap_or("");
            format!("  {}: {} // {}", c.name, c.signature, doc_hint)
        })
        .collect();

    let trait_list: Vec<String> = impls
        .iter()
        .map(|r| format!("  {} ({})", r.symbol.name, r.ref_kind))
        .collect();

    let doc_str = symbol.doc_comment.as_deref().unwrap_or("(no doc comment)");

    let context = format!(
        "Type: {} ({})\nSignature: {}\nDoc comment:\n{}\n\nMethods ({}):\n{}\n\nTrait relationships ({}):\n{}",
        symbol.name,
        symbol.kind,
        symbol.signature,
        doc_str,
        methods.len(),
        methods.join("\n"),
        trait_list.len(),
        trait_list.join("\n"),
    );

    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a technical documentation expert. Given a Rust type with its methods and trait \
             relationships, produce a concise summary (1-3 paragraphs) that explains what this type \
             represents, its key methods, and how it fits into the library. Be precise.",
        ),
        CompletionMessage::new(Role::User, &context),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(1000))
        .await
        .map_err(|e| anyhow::anyhow!("LLM error: {e}"))?;

    Ok(resp.content)
}

/// Run summary generation for a given scope, store in DB, return the content.
pub async fn generate_and_store_summary(
    client: Arc<OpenAiCompatibleClient>,
    db: &Database,
    snapshot_id: i64,
    scope: &str,
) -> Result<String> {
    let content = match scope {
        "crate" => generate_crate_summary(&client, db, snapshot_id).await?,
        s if s.starts_with("module:") => {
            let module_path = s.strip_prefix("module:").unwrap();
            generate_module_summary(&client, db, snapshot_id, module_path).await?
        }
        s if s.starts_with("type:") => {
            let symbol_id: i64 = s
                .strip_prefix("type:")
                .unwrap()
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid symbol_id in scope"))?;
            generate_type_summary(&client, db, snapshot_id, symbol_id).await?
        }
        _ => return Err(anyhow::anyhow!("invalid scope: {scope}. Use 'crate', 'module:<path>', or 'type:<symbol_id>'")),
    };

    let model_name = client.name();
    db.upsert_summary(snapshot_id, scope, &content, model_name)
        .await?;

    Ok(content)
}
