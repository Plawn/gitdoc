use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::db::{Database, EmbeddingInsert, SymbolInsert};
use crate::embeddings::{self, EmbeddingProvider};
use crate::search::{SearchIndex, SymbolSearchEntry};
use super::doc_parser;
use super::git_walker;
use super::reference_resolver;
use super::ts_parser::{self, SourceLanguage};

#[derive(Debug, serde::Serialize)]
pub struct IndexResult {
    pub snapshot_id: i64,
    pub files_scanned: i64,
    pub docs_count: i64,
    pub symbols_count: i64,
    pub refs_count: usize,
    pub embeddings_count: usize,
    pub duration_ms: u64,
}

const EMBEDDING_BATCH_SIZE: usize = 96;

struct PendingEmbedding {
    file_id: i64,
    source_type: String,
    source_id: i64,
    text: String,
}

pub async fn run_indexation(
    db: &Arc<Database>,
    search: &SearchIndex,
    repo_id: &str,
    repo_path: &Path,
    ref_str: &str,
    label: Option<&str>,
    embedder: Option<Arc<dyn EmbeddingProvider>>,
    exclusion_patterns: &[String],
) -> Result<IndexResult> {
    let start = Instant::now();

    // Step 1: Resolve commit (CPU-bound git work)
    let repo_path_buf = repo_path.to_path_buf();
    let ref_str_owned = ref_str.to_string();
    let commit_sha = tokio::task::spawn_blocking(move || {
        git_walker::resolve_commit(&repo_path_buf, &ref_str_owned)
    })
    .await?
    .context("failed to resolve commit")?;

    // Step 2: Check if already indexed
    if let Some(existing_id) = db.find_snapshot(repo_id, &commit_sha).await? {
        tracing::info!(snapshot_id = existing_id, "commit already indexed, returning existing snapshot");
        let files = db.count_files_for_snapshot(existing_id).await?;
        let docs = db.count_docs_for_snapshot(existing_id).await?;
        let syms = db.count_symbols_for_snapshot(existing_id).await?;
        return Ok(IndexResult {
            snapshot_id: existing_id,
            files_scanned: files,
            docs_count: docs,
            symbols_count: syms,
            refs_count: 0,
            embeddings_count: 0,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    // Step 3: Create snapshot
    let snapshot_id = db.create_snapshot(repo_id, &commit_sha, label).await?;

    // Step 4: Walk files (CPU-bound git work)
    let repo_path_buf = repo_path.to_path_buf();
    let commit_sha_clone = commit_sha.clone();
    let files = match tokio::task::spawn_blocking(move || {
        git_walker::walk_commit(&repo_path_buf, &commit_sha_clone)
    })
    .await?
    {
        Ok(f) => f,
        Err(e) => {
            db.fail_snapshot(snapshot_id, &e.to_string()).await?;
            return Err(e);
        }
    };

    let mut files_scanned: i64 = 0;
    let mut source_contents: HashMap<i64, Vec<u8>> = HashMap::new();
    let mut source_types: HashMap<i64, String> = HashMap::new();
    let mut source_paths: HashMap<i64, String> = HashMap::new();
    let mut pending_embeddings: Vec<PendingEmbedding> = Vec::new();

    for file_entry in &files {
        if exclusion_patterns.iter().any(|p| file_entry.path.starts_with(p.as_str())) {
            continue;
        }

        files_scanned += 1;

        let file_type = classify_file(&file_entry.path);

        let file_id = match db.find_file_by_checksum(&file_entry.checksum).await? {
            Some(id) => id,
            None => {
                let content = if file_type == "doc" {
                    Some(std::str::from_utf8(&file_entry.content).unwrap_or(""))
                } else {
                    None
                };
                db.insert_file(&file_entry.checksum, content).await?
            }
        };

        db.insert_snapshot_file(snapshot_id, &file_entry.path, file_id, file_type).await?;

        match file_type {
            "rust" | "typescript" | "tsx" | "javascript" => {
                source_contents.insert(file_id, file_entry.content.clone());
                source_types.insert(file_id, file_type.to_string());
                source_paths.insert(file_id, file_entry.path.clone());
            }
            _ => {}
        }

        match file_type {
            "doc" => {
                if !db.doc_exists_for_file(file_id).await? {
                    let content = std::str::from_utf8(&file_entry.content).unwrap_or("");
                    let doc_info = doc_parser::parse_doc(content);
                    let doc_id = db.insert_doc(file_id, doc_info.title.as_deref()).await?;
                    search.add_doc(file_id, &file_entry.path, doc_info.title.as_deref(), content)?;

                    if embedder.is_some() && !db.embeddings_exist_for_file(file_id).await? {
                        for chunk in &doc_info.chunks {
                            pending_embeddings.push(PendingEmbedding {
                                file_id,
                                source_type: "doc_chunk".into(),
                                source_id: doc_id,
                                text: chunk.text.clone(),
                            });
                        }
                    }
                }
            }
            "rust" | "typescript" | "tsx" | "javascript" => {
                if !db.symbols_exist_for_file(file_id).await? {
                    let lang = match file_type {
                        "rust" => SourceLanguage::Rust,
                        "typescript" => SourceLanguage::TypeScript,
                        "tsx" => SourceLanguage::Tsx,
                        "javascript" => SourceLanguage::JavaScript,
                        _ => unreachable!(),
                    };

                    // Parse file (CPU-bound tree-sitter work)
                    let content = file_entry.content.clone();
                    let path = file_entry.path.clone();
                    let symbols = tokio::task::spawn_blocking(move || {
                        ts_parser::parse_file(&content, lang, &path)
                    })
                    .await?;

                    let parent_indices: Vec<Option<usize>> = symbols
                        .iter()
                        .enumerate()
                        .map(|(i, sym)| {
                            let mut best: Option<(usize, usize)> = None;
                            for (j, other) in symbols.iter().enumerate() {
                                if j == i {
                                    continue;
                                }
                                if other.byte_start <= sym.byte_start
                                    && other.byte_end >= sym.byte_end
                                    && (other.byte_start != sym.byte_start
                                        || other.byte_end != sym.byte_end)
                                {
                                    let span = other.byte_end - other.byte_start;
                                    if best.is_none() || span < best.unwrap().1 {
                                        best = Some((j, span));
                                    }
                                }
                            }
                            best.map(|(idx, _)| idx)
                        })
                        .collect();

                    let mut db_ids: Vec<i64> = Vec::with_capacity(symbols.len());
                    for sym in &symbols {
                        let qualified_name = format!("{}::{}", file_entry.path, sym.name);
                        let id = db.insert_symbol(&SymbolInsert {
                            file_id,
                            name: sym.name.clone(),
                            qualified_name,
                            kind: sym.kind.clone(),
                            visibility: sym.visibility.clone(),
                            file_path: file_entry.path.clone(),
                            line_start: sym.line_start as i64,
                            line_end: sym.line_end as i64,
                            byte_start: sym.byte_start as i64,
                            byte_end: sym.byte_end as i64,
                            parent_id: None,
                            signature: sym.signature.clone(),
                            doc_comment: sym.doc_comment.clone(),
                            body: sym.body.clone(),
                        }).await?;
                        db_ids.push(id);
                    }

                    for (i, parent_idx) in parent_indices.iter().enumerate() {
                        if let Some(pi) = parent_idx {
                            db.update_symbol_parent(db_ids[i], db_ids[*pi]).await?;
                        }
                    }

                    let db_symbols = db.get_symbols_for_file(file_id).await?;
                    let entries: Vec<SymbolSearchEntry> = db_symbols.iter().map(|s| {
                        SymbolSearchEntry {
                            file_id,
                            symbol_id: s.id,
                            name: s.name.clone(),
                            qualified_name: s.qualified_name.clone(),
                            kind: s.kind.clone(),
                            visibility: s.visibility.clone(),
                            signature: s.signature.clone(),
                            doc_comment: s.doc_comment.clone(),
                            file_path: s.file_path.clone(),
                        }
                    }).collect();
                    search.add_symbols(&entries)?;

                    if embedder.is_some() && !db.embeddings_exist_for_file(file_id).await? {
                        for s in &db_symbols {
                            let mut text = format!("{} {}: {}", s.kind, s.name, s.signature);
                            if let Some(ref doc) = s.doc_comment {
                                text.push('\n');
                                text.push_str(doc);
                            }
                            pending_embeddings.push(PendingEmbedding {
                                file_id,
                                source_type: "symbol".into(),
                                source_id: s.id,
                                text,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Step 5: Commit search indexes
    search.commit_all()?;

    // Step 6: Resolve references (CPU-bound)
    let symbols_for_refs = db.get_symbols_with_bodies_for_snapshot(snapshot_id).await?;
    let detected_refs = tokio::task::spawn_blocking(move || {
        reference_resolver::resolve_references(
            &symbols_for_refs,
            &source_contents,
            &source_types,
            &source_paths,
        )
    })
    .await?;
    let ref_tuples: Vec<(i64, i64, &str)> = detected_refs
        .iter()
        .map(|r| (r.from_symbol_id, r.to_symbol_id, r.kind.as_str()))
        .collect();
    let refs_count = if !ref_tuples.is_empty() {
        db.insert_refs_batch(&ref_tuples).await?
    } else {
        0
    };
    tracing::info!(refs_count, "reference resolution complete");

    // Step 7: Generate embeddings
    let mut embeddings_count = 0;
    if let Some(ref embedder) = embedder {
        if !pending_embeddings.is_empty() {
            tracing::info!(count = pending_embeddings.len(), "generating embeddings");
            for batch_start in (0..pending_embeddings.len()).step_by(EMBEDDING_BATCH_SIZE) {
                let batch_end = (batch_start + EMBEDDING_BATCH_SIZE).min(pending_embeddings.len());
                let batch = &pending_embeddings[batch_start..batch_end];
                let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();

                match embedder.embed_batch(&texts).await {
                    Ok(vectors) => {
                        let inserts: Vec<EmbeddingInsert> = batch
                            .iter()
                            .zip(vectors.iter())
                            .map(|(p, vec)| EmbeddingInsert {
                                file_id: p.file_id,
                                source_type: p.source_type.clone(),
                                source_id: p.source_id,
                                text: p.text.clone(),
                                vector: Some(embeddings::to_pgvector(vec)),
                            })
                            .collect();
                        embeddings_count += db.insert_embeddings_batch(&inserts).await?;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "embedding batch failed, skipping");
                    }
                }
            }
            tracing::info!(embeddings_count, "embeddings generated");
        }
    }

    // Step 8: Finalize
    let docs_count = db.count_docs_for_snapshot(snapshot_id).await?;
    let symbols_count = db.count_symbols_for_snapshot(snapshot_id).await?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let stats = serde_json::json!({
        "files_scanned": files_scanned,
        "docs_count": docs_count,
        "symbols_count": symbols_count,
        "refs_count": refs_count,
        "embeddings_count": embeddings_count,
        "duration_ms": duration_ms,
    });
    db.finalize_snapshot(snapshot_id, &stats.to_string()).await?;

    Ok(IndexResult {
        snapshot_id,
        files_scanned,
        docs_count,
        symbols_count,
        refs_count,
        embeddings_count,
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
