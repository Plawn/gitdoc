use std::sync::Arc;

use anyhow::Result;
use llm_ai::{CompletionMessage, OpenAiCompatibleClient, Role};

use crate::db::Database;
use crate::llm_executor::{LlmExecutor, PROMPT_SUMMARY_CRATE, PROMPT_SUMMARY_MODULE, PROMPT_SUMMARY_TYPE};

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
            let mod_path = crate::util::path_to_module(&f.file_path);
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

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &context)];
    let resp = executor.run_anyhow(&PROMPT_SUMMARY_CRATE, &user).await?;

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

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &context)];
    let resp = executor.run_anyhow(&PROMPT_SUMMARY_MODULE, &user).await?;

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

    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &context)];
    let resp = executor.run_anyhow(&PROMPT_SUMMARY_TYPE, &user).await?;

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

/// Shared `synthesize_answer` used by both `api/explain.rs` and `grpc/analysis.rs`.
pub async fn synthesize_answer(
    client: &OpenAiCompatibleClient,
    query: &str,
    symbols: &[gitdoc_api_types::responses::RelevantSymbol],
    docs: &[gitdoc_api_types::responses::RelevantDoc],
) -> Result<String> {
    let mut context = String::new();

    if !docs.is_empty() {
        context.push_str("## Relevant documentation\n\n");
        for doc in docs.iter().take(5) {
            context.push_str(&format!(
                "### {} ({})\n{}\n\n",
                doc.title.as_deref().unwrap_or("untitled"),
                doc.file_path,
                doc.snippet,
            ));
        }
    }

    if !symbols.is_empty() {
        context.push_str("## Relevant symbols\n\n");
        for sym in symbols.iter().take(10) {
            context.push_str(&format!(
                "### {} ({}) — {}\n",
                sym.name, sym.kind, sym.file_path
            ));
            context.push_str(&format!("Signature: {}\n", sym.signature));
            if let Some(ref doc) = sym.doc_comment {
                let first_lines: String = doc.lines().take(5).collect::<Vec<_>>().join("\n");
                context.push_str(&format!("Doc: {}\n", first_lines));
            }
            if !sym.methods.is_empty() {
                context.push_str("Methods:\n");
                for m in sym.methods.iter().take(10) {
                    context.push_str(&format!("  - {}: {}\n", m.name, m.signature));
                }
            }
            if !sym.traits.is_empty() {
                context.push_str(&format!("Implements: {}\n", sym.traits.join(", ")));
            }
            context.push('\n');
        }
    }

    let user_msg = format!("Question: {}\n\n{}", query, context);
    let executor = LlmExecutor::new(client);
    let user = [CompletionMessage::new(Role::User, &user_msg)];
    let resp = executor.run_anyhow(&crate::llm_executor::PROMPT_EXPLAIN, &user).await?;

    Ok(resp.content)
}
