use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::db::{Database, SymbolInsert};
use super::doc_parser;
use super::git_walker;
use super::reference_resolver;
use super::ts_parser::{self, SourceLanguage};

const EXCLUDED_PREFIXES: &[&str] = &[
    "node_modules/",
    "target/",
    ".git/",
    "vendor/",
    ".next/",
    "dist/",
    "build/",
    "__pycache__/",
];

#[derive(Debug, serde::Serialize)]
pub struct IndexResult {
    pub snapshot_id: i64,
    pub files_scanned: i64,
    pub docs_count: i64,
    pub symbols_count: i64,
    pub refs_count: usize,
    pub duration_ms: u64,
}

pub fn run_indexation(
    db: &Arc<Database>,
    repo_id: &str,
    repo_path: &Path,
    ref_str: &str,
    label: Option<&str>,
) -> Result<IndexResult> {
    let start = Instant::now();

    // Step 1: Resolve commit
    let commit_sha = git_walker::resolve_commit(repo_path, ref_str)
        .context("failed to resolve commit")?;

    // Step 2: Check if already indexed
    if let Some(existing_id) = db.find_snapshot(repo_id, &commit_sha)? {
        tracing::info!(snapshot_id = existing_id, "commit already indexed, returning existing snapshot");
        let files = db.count_files_for_snapshot(existing_id)?;
        let docs = db.count_docs_for_snapshot(existing_id)?;
        let syms = db.count_symbols_for_snapshot(existing_id)?;
        return Ok(IndexResult {
            snapshot_id: existing_id,
            files_scanned: files,
            docs_count: docs,
            symbols_count: syms,
            refs_count: 0,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    // Step 3: Create snapshot
    let snapshot_id = db.create_snapshot(repo_id, &commit_sha, label)?;

    // Step 4: Walk files
    let files = match git_walker::walk_commit(repo_path, &commit_sha) {
        Ok(f) => f,
        Err(e) => {
            db.fail_snapshot(snapshot_id, &e.to_string())?;
            return Err(e);
        }
    };

    let mut files_scanned: i64 = 0;
    // Collect source file contents for reference resolution (file_id → content)
    let mut source_contents: HashMap<i64, Vec<u8>> = HashMap::new();
    let mut source_types: HashMap<i64, String> = HashMap::new();
    let mut source_paths: HashMap<i64, String> = HashMap::new();

    for file_entry in &files {
        // Exclude paths
        if EXCLUDED_PREFIXES.iter().any(|p| file_entry.path.starts_with(p)) {
            continue;
        }

        files_scanned += 1;

        // Classify file type
        let file_type = classify_file(&file_entry.path);

        // Deduplicate via checksum
        let file_id = match db.find_file_by_checksum(&file_entry.checksum)? {
            Some(id) => id,
            None => {
                // For docs, store content; for source, store None (content is in symbols)
                let content = if file_type == "doc" {
                    Some(std::str::from_utf8(&file_entry.content).unwrap_or(""))
                } else {
                    None
                };
                db.insert_file(&file_entry.checksum, content)?
            }
        };

        // Link file to snapshot
        db.insert_snapshot_file(snapshot_id, &file_entry.path, file_id, file_type)?;

        // Track source files for reference resolution
        match file_type {
            "rust" | "typescript" | "tsx" | "javascript" => {
                source_contents.insert(file_id, file_entry.content.clone());
                source_types.insert(file_id, file_type.to_string());
                source_paths.insert(file_id, file_entry.path.clone());
            }
            _ => {}
        }

        // Parse new files only (if file_id already has docs/symbols, skip)
        match file_type {
            "doc" => {
                if !db.doc_exists_for_file(file_id)? {
                    let content = std::str::from_utf8(&file_entry.content).unwrap_or("");
                    let doc_info = doc_parser::parse_doc(content);
                    db.insert_doc(file_id, doc_info.title.as_deref())?;
                }
            }
            "rust" | "typescript" | "tsx" | "javascript" => {
                if !db.symbols_exist_for_file(file_id)? {
                    let lang = match file_type {
                        "rust" => SourceLanguage::Rust,
                        "typescript" => SourceLanguage::TypeScript,
                        "tsx" => SourceLanguage::Tsx,
                        "javascript" => SourceLanguage::JavaScript,
                        _ => unreachable!(),
                    };
                    let symbols = ts_parser::parse_file(&file_entry.content, lang, &file_entry.path);
                    for sym in symbols {
                        let qualified_name = format!("{}::{}", file_entry.path, sym.name);
                        db.insert_symbol(&SymbolInsert {
                            file_id,
                            name: sym.name,
                            qualified_name,
                            kind: sym.kind,
                            visibility: sym.visibility,
                            file_path: file_entry.path.clone(),
                            line_start: sym.line_start as i64,
                            line_end: sym.line_end as i64,
                            byte_start: sym.byte_start as i64,
                            byte_end: sym.byte_end as i64,
                            parent_id: None,
                            signature: sym.signature,
                            doc_comment: sym.doc_comment,
                            body: sym.body,
                        })?;
                    }
                }
            }
            _ => {}
        }
    }

    // Step 5: Resolve references
    let symbols_for_refs = db.get_symbols_with_bodies_for_snapshot(snapshot_id)?;
    let detected_refs = reference_resolver::resolve_references(
        &symbols_for_refs,
        &source_contents,
        &source_types,
        &source_paths,
    );
    let ref_tuples: Vec<(i64, i64, &str)> = detected_refs
        .iter()
        .map(|r| (r.from_symbol_id, r.to_symbol_id, r.kind.as_str()))
        .collect();
    let refs_count = if !ref_tuples.is_empty() {
        db.insert_refs_batch(&ref_tuples)?
    } else {
        0
    };
    tracing::info!(refs_count, "reference resolution complete");

    // Step 6: Finalize
    let docs_count = db.count_docs_for_snapshot(snapshot_id)?;
    let symbols_count = db.count_symbols_for_snapshot(snapshot_id)?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let stats = serde_json::json!({
        "files_scanned": files_scanned,
        "docs_count": docs_count,
        "symbols_count": symbols_count,
        "refs_count": refs_count,
        "duration_ms": duration_ms,
    });
    db.finalize_snapshot(snapshot_id, &stats.to_string())?;

    Ok(IndexResult {
        snapshot_id,
        files_scanned,
        docs_count,
        symbols_count,
        refs_count,
        duration_ms,
    })
}

fn classify_file(path: &str) -> &'static str {
    if path.ends_with(".md") || path.ends_with(".mdx") {
        "doc"
    } else if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".ts") && !path.ends_with(".d.ts") {
        "typescript"
    } else if path.ends_with(".tsx") {
        "tsx"
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        "javascript"
    } else {
        "other"
    }
}
