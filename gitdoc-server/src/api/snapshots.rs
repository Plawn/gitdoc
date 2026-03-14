use r2e::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use gitdoc_api_types::responses::{
    OverviewSymbol, DiffSymbolEntry, DiffSigVis, DiffSummary,
    DiffResponse, DiffSummarizeResponse, ModifiedSymbol,
};

use crate::AppState;
use crate::db::SymbolFilters;
use crate::error::GitdocError;
use crate::llm_executor::{LlmExecutor, PROMPT_DIFF_SUMMARIZE};

pub use crate::util::path_to_module;

/// Server-local: embeds db types directly for zero-copy serialization.
/// JSON shape matches `gitdoc_api_types::responses::OverviewResponse`.
#[derive(Serialize)]
pub struct OverviewResponse {
    pub snapshot: crate::db::SnapshotRow,
    pub readme: Option<String>,
    pub docs: Vec<crate::db::DocRow>,
    pub top_level_symbols: Vec<OverviewSymbol>,
}

#[derive(Serialize)]
pub struct DeleteSnapshotResponse {
    pub deleted: bool,
    pub gc: crate::db::GcStats,
}

use gitdoc_api_types::requests::DiffQuery;

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct SnapshotController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl SnapshotController {
    #[get("/{snapshot_id}/overview")]
    async fn get_overview(
        &self,
        Path(snapshot_id): Path<i64>,
    ) -> Result<Json<OverviewResponse>, GitdocError> {
        let snapshot = self.db.get_snapshot(snapshot_id).await?
            .ok_or_else(|| GitdocError::NotFound("snapshot not found".into()))?;

        let docs = self.db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_else(|e| {
            tracing::warn!(snapshot_id, error = %e, "failed to list docs for snapshot");
            Vec::new()
        });
        let readme = docs.iter().find(|d| {
            let lower = d.file_path.to_lowercase();
            lower == "readme.md" || lower.ends_with("/readme.md")
        });
        let readme_content = if let Some(r) = readme {
            match self.db.get_doc_content(snapshot_id, &r.file_path).await {
                Ok(dc) => dc.and_then(|d| d.content),
                Err(e) => {
                    tracing::warn!(snapshot_id, file_path = %r.file_path, error = %e, "failed to load readme content");
                    None
                }
            }
        } else {
            None
        };

        let symbols = self
            .db
            .list_symbols_for_snapshot(
                snapshot_id,
                &SymbolFilters {
                    include_private: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(snapshot_id, error = %e, "failed to list symbols for overview");
                Vec::new()
            });
        let top_level_symbols: Vec<_> = symbols
            .iter()
            .filter(|s| s.parent_id.is_none())
            .map(|s| OverviewSymbol {
                id: s.id,
                name: s.name.clone(),
                qualified_name: s.qualified_name.clone(),
                kind: s.kind.clone(),
                visibility: s.visibility.clone(),
                file_path: s.file_path.clone(),
                signature: s.signature.clone(),
                doc_comment: s.doc_comment.clone(),
            })
            .collect();

        Ok(Json(OverviewResponse {
            snapshot,
            readme: readme_content,
            docs,
            top_level_symbols,
        }))
    }

    #[get("/{snapshot_id}/docs")]
    async fn list_docs(
        &self,
        Path(snapshot_id): Path<i64>,
    ) -> Result<Json<Vec<crate::db::DocRow>>, GitdocError> {
        let docs = self.db.list_docs_for_snapshot(snapshot_id).await?;
        Ok(Json(docs))
    }

    #[get("/{snapshot_id}/docs/{*path}")]
    async fn get_doc_content(
        &self,
        Path((snapshot_id, path)): Path<(i64, String)>,
    ) -> Result<Json<crate::db::DocContent>, GitdocError> {
        let doc = self.db.get_doc_content(snapshot_id, &path).await?
            .ok_or_else(|| GitdocError::NotFound("doc not found".into()))?;
        Ok(Json(doc))
    }

    #[get("/{from_id}/diff/{to_id}")]
    async fn diff_symbols(
        &self,
        Path((from_id, to_id)): Path<(i64, i64)>,
        Query(q): Query<DiffQuery>,
    ) -> Result<Json<DiffResponse>, GitdocError> {
        let include_private = q.include_private.unwrap_or(false);
        let include_source = q.include_source.unwrap_or(false);
        let filters = SymbolFilters {
            kind: q.kind.clone(),
            include_private,
            ..Default::default()
        };

        let from_symbols = self.db.list_symbols_for_snapshot(from_id, &filters).await?;
        let to_symbols = self.db.list_symbols_for_snapshot(to_id, &filters).await?;

        let from_map: HashMap<&str, &crate::db::SymbolRow> = from_symbols
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();
        let to_map: HashMap<&str, &crate::db::SymbolRow> = to_symbols
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();

        for sym in &to_symbols {
            if !from_map.contains_key(sym.qualified_name.as_str()) {
                added.push(DiffSymbolEntry {
                    name: sym.name.clone(),
                    qualified_name: sym.qualified_name.clone(),
                    kind: sym.kind.clone(),
                    visibility: sym.visibility.clone(),
                    file_path: sym.file_path.clone(),
                    signature: sym.signature.clone(),
                    body: None,
                });
            }
        }

        for sym in &from_symbols {
            if !to_map.contains_key(sym.qualified_name.as_str()) {
                removed.push(DiffSymbolEntry {
                    name: sym.name.clone(),
                    qualified_name: sym.qualified_name.clone(),
                    kind: sym.kind.clone(),
                    visibility: sym.visibility.clone(),
                    file_path: sym.file_path.clone(),
                    signature: sym.signature.clone(),
                    body: None,
                });
            }
        }

        for sym in &to_symbols {
            if let Some(from_sym) = from_map.get(sym.qualified_name.as_str()) {
                let mut changes = Vec::new();
                if from_sym.signature != sym.signature {
                    changes.push("signature".to_string());
                }
                if from_sym.visibility != sym.visibility {
                    changes.push("visibility".to_string());
                }
                if !changes.is_empty() {
                    modified.push(ModifiedSymbol {
                        qualified_name: sym.qualified_name.clone(),
                        kind: sym.kind.clone(),
                        changes,
                        from: DiffSigVis {
                            signature: from_sym.signature.clone(),
                            visibility: from_sym.visibility.clone(),
                            body: None,
                        },
                        to: DiffSigVis {
                            signature: sym.signature.clone(),
                            visibility: sym.visibility.clone(),
                            body: None,
                        },
                    });
                }
            }
        }

        if include_source {
            let added_qnames: Vec<&str> = added.iter().map(|s| s.qualified_name.as_str()).collect();
            let removed_qnames: Vec<&str> = removed.iter().map(|s| s.qualified_name.as_str()).collect();
            let modified_qnames: Vec<&str> = modified.iter().map(|s| s.qualified_name.as_str()).collect();

            let mut to_qnames = added_qnames.clone();
            to_qnames.extend(&modified_qnames);
            let to_details = self.db.get_symbols_with_body_for_snapshot_by_qnames(to_id, &to_qnames).await?;
            let to_body_map: HashMap<&str, &str> = to_details.iter().map(|s| (s.qualified_name.as_str(), s.body.as_str())).collect();

            let mut from_qnames = removed_qnames.clone();
            from_qnames.extend(&modified_qnames);
            let from_details = self.db.get_symbols_with_body_for_snapshot_by_qnames(from_id, &from_qnames).await?;
            let from_body_map: HashMap<&str, &str> = from_details.iter().map(|s| (s.qualified_name.as_str(), s.body.as_str())).collect();

            for entry in &mut added {
                entry.body = to_body_map.get(entry.qualified_name.as_str()).map(|b| b.to_string());
            }
            for entry in &mut removed {
                entry.body = from_body_map.get(entry.qualified_name.as_str()).map(|b| b.to_string());
            }
            for entry in &mut modified {
                entry.from.body = from_body_map.get(entry.qualified_name.as_str()).map(|b| b.to_string());
                entry.to.body = to_body_map.get(entry.qualified_name.as_str()).map(|b| b.to_string());
            }
        }

        let summary = DiffSummary {
            added: added.len(),
            removed: removed.len(),
            modified: modified.len(),
        };

        Ok(Json(DiffResponse {
            from_snapshot: from_id,
            to_snapshot: to_id,
            added,
            removed,
            modified,
            summary,
        }))
    }

    #[post("/{from_id}/diff/{to_id}/summarize")]
    async fn diff_summarize(
        &self,
        Path((from_id, to_id)): Path<(i64, i64)>,
        Query(q): Query<DiffQuery>,
    ) -> Result<Json<DiffSummarizeResponse>, GitdocError> {
        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| GitdocError::ServiceUnavailable("no LLM provider configured".into()))?;

        let include_private = q.include_private.unwrap_or(false);
        let filters = SymbolFilters {
            kind: q.kind.clone(),
            include_private,
            ..Default::default()
        };

        let from_symbols = self.db.list_symbols_for_snapshot(from_id, &filters).await?;
        let to_symbols = self.db.list_symbols_for_snapshot(to_id, &filters).await?;

        let from_map: HashMap<&str, &crate::db::SymbolRow> = from_symbols
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();
        let to_map: HashMap<&str, &crate::db::SymbolRow> = to_symbols
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();

        let mut added_lines = Vec::new();
        let mut removed_lines = Vec::new();
        let mut modified_lines = Vec::new();

        for sym in &to_symbols {
            if !from_map.contains_key(sym.qualified_name.as_str()) {
                added_lines.push(format!("+ {} {} ({})", sym.kind, sym.qualified_name, sym.signature));
            }
        }
        for sym in &from_symbols {
            if !to_map.contains_key(sym.qualified_name.as_str()) {
                removed_lines.push(format!("- {} {} ({})", sym.kind, sym.qualified_name, sym.signature));
            }
        }
        for sym in &to_symbols {
            if let Some(from_sym) = from_map.get(sym.qualified_name.as_str()) {
                let mut changes = Vec::new();
                if from_sym.signature != sym.signature {
                    changes.push(format!("signature: {} → {}", from_sym.signature, sym.signature));
                }
                if from_sym.visibility != sym.visibility {
                    changes.push(format!("visibility: {} → {}", from_sym.visibility, sym.visibility));
                }
                if !changes.is_empty() {
                    modified_lines.push(format!("~ {} {}: {}", sym.kind, sym.qualified_name, changes.join(", ")));
                }
            }
        }

        let stats = DiffSummary {
            added: added_lines.len(),
            removed: removed_lines.len(),
            modified: modified_lines.len(),
        };

        let mut diff_text = String::new();
        if !added_lines.is_empty() {
            diff_text.push_str("ADDED:\n");
            for l in &added_lines { diff_text.push_str(l); diff_text.push('\n'); }
        }
        if !removed_lines.is_empty() {
            diff_text.push_str("\nREMOVED:\n");
            for l in &removed_lines { diff_text.push_str(l); diff_text.push('\n'); }
        }
        if !modified_lines.is_empty() {
            diff_text.push_str("\nMODIFIED:\n");
            for l in &modified_lines { diff_text.push_str(l); diff_text.push('\n'); }
        }

        if diff_text.is_empty() {
            return Ok(Json(DiffSummarizeResponse {
                from_snapshot: from_id,
                to_snapshot: to_id,
                changelog: "No changes detected between these snapshots.".to_string(),
                stats,
            }));
        }

        let user_content = format!(
            "Generate a changelog for the following diff ({} added, {} removed, {} modified symbols):\n\n{}",
            stats.added, stats.removed, stats.modified, diff_text
        );

        let executor = LlmExecutor::new(&llm_client);
        let user_msgs = [llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_content)];
        let response = executor.run(&PROMPT_DIFF_SUMMARIZE, &user_msgs)
            .await
            .map_err(|e| GitdocError::Internal(e.into()))?;
        let changelog = response.content;

        Ok(Json(DiffSummarizeResponse {
            from_snapshot: from_id,
            to_snapshot: to_id,
            changelog,
            stats,
        }))
    }

    #[delete("/{snapshot_id}")]
    async fn delete_snapshot(
        &self,
        Path(snapshot_id): Path<i64>,
    ) -> Result<Json<DeleteSnapshotResponse>, GitdocError> {
        let existed = self.db.delete_snapshot(snapshot_id).await?;
        if !existed {
            return Err(GitdocError::NotFound("snapshot not found".into()));
        }
        let gc = self.db.gc_orphans().await?;
        Ok(Json(DeleteSnapshotResponse { deleted: true, gc }))
    }
}
