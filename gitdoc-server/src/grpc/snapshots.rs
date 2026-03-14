use r2e::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use gitdoc_api_types::responses::{
    DiffSigVis, DiffSummary, DiffSymbolEntry, ModifiedSymbol, OverviewSymbol,
};

use super::proto;
use crate::db::SymbolFilters;
use crate::AppState;
use crate::llm_executor::{LlmExecutor, PROMPT_DIFF_SUMMARIZE};

#[derive(Controller)]
#[controller(state = AppState)]
pub struct SnapshotGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[grpc_routes(proto::snapshot_service_server::SnapshotService)]
impl SnapshotGrpcService {
    async fn get_overview(
        &self,
        request: Request<proto::GetOverviewRequest>,
    ) -> Result<Response<proto::GetOverviewResponse>, Status> {
        let snapshot_id = request.into_inner().snapshot_id;
        let snapshot = self
            .db
            .get_snapshot(snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("snapshot not found"))?;

        let docs = self
            .db
            .list_docs_for_snapshot(snapshot_id)
            .await
            .unwrap_or_default();
        let readme = docs.iter().find(|d| {
            let lower = d.file_path.to_lowercase();
            lower == "readme.md" || lower.ends_with("/readme.md")
        });
        let readme_content = if let Some(r) = readme {
            self.db
                .get_doc_content(snapshot_id, &r.file_path)
                .await
                .ok()
                .flatten()
                .and_then(|d| d.content)
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
            .unwrap_or_default();
        let top_level: Vec<proto::OverviewSymbol> = symbols
            .iter()
            .filter(|s| s.parent_id.is_none())
            .map(|s| {
                OverviewSymbol {
                    id: s.id,
                    name: s.name.clone(),
                    qualified_name: s.qualified_name.clone(),
                    kind: s.kind.clone(),
                    visibility: s.visibility.clone(),
                    file_path: s.file_path.clone(),
                    signature: s.signature.clone(),
                    doc_comment: s.doc_comment.clone(),
                }
                .into()
            })
            .collect();

        Ok(Response::new(proto::GetOverviewResponse {
            snapshot: Some(snapshot.into()),
            readme: readme_content.unwrap_or_default(),
            docs: docs.into_iter().map(Into::into).collect(),
            top_level_symbols: top_level,
        }))
    }

    async fn list_docs(
        &self,
        request: Request<proto::ListDocsRequest>,
    ) -> Result<Response<proto::ListDocsResponse>, Status> {
        let snapshot_id = request.into_inner().snapshot_id;
        let docs = self
            .db
            .list_docs_for_snapshot(snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListDocsResponse {
            docs: docs.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_doc_content(
        &self,
        request: Request<proto::GetDocContentRequest>,
    ) -> Result<Response<proto::GetDocContentResponse>, Status> {
        let req = request.into_inner();
        let doc = self
            .db
            .get_doc_content(req.snapshot_id, &req.path)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("doc not found"))?;
        Ok(Response::new(proto::GetDocContentResponse {
            doc: Some(doc.into()),
        }))
    }

    async fn diff_snapshots(
        &self,
        request: Request<proto::DiffSnapshotsRequest>,
    ) -> Result<Response<proto::DiffSnapshotsResponse>, Status> {
        let req = request.into_inner();
        let filters = SymbolFilters {
            kind: if req.kind.is_empty() {
                None
            } else {
                Some(req.kind)
            },
            include_private: req.include_private,
            ..Default::default()
        };

        let from_symbols = self
            .db
            .list_symbols_for_snapshot(req.from_id, &filters)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let to_symbols = self
            .db
            .list_symbols_for_snapshot(req.to_id, &filters)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

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

        if req.include_source {
            let added_qnames: Vec<&str> =
                added.iter().map(|s| s.qualified_name.as_str()).collect();
            let removed_qnames: Vec<&str> =
                removed.iter().map(|s| s.qualified_name.as_str()).collect();
            let modified_qnames: Vec<&str> =
                modified.iter().map(|s| s.qualified_name.as_str()).collect();

            let mut to_qnames = added_qnames.clone();
            to_qnames.extend(&modified_qnames);
            let to_details = self
                .db
                .get_symbols_with_body_for_snapshot_by_qnames(req.to_id, &to_qnames)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let to_body_map: HashMap<&str, &str> = to_details
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.body.as_str()))
                .collect();

            let mut from_qnames = removed_qnames.clone();
            from_qnames.extend(&modified_qnames);
            let from_details = self
                .db
                .get_symbols_with_body_for_snapshot_by_qnames(req.from_id, &from_qnames)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let from_body_map: HashMap<&str, &str> = from_details
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.body.as_str()))
                .collect();

            for entry in &mut added {
                entry.body = to_body_map
                    .get(entry.qualified_name.as_str())
                    .map(|b| b.to_string());
            }
            for entry in &mut removed {
                entry.body = from_body_map
                    .get(entry.qualified_name.as_str())
                    .map(|b| b.to_string());
            }
            for entry in &mut modified {
                entry.from.body = from_body_map
                    .get(entry.qualified_name.as_str())
                    .map(|b| b.to_string());
                entry.to.body = to_body_map
                    .get(entry.qualified_name.as_str())
                    .map(|b| b.to_string());
            }
        }

        let summary = DiffSummary {
            added: added.len(),
            removed: removed.len(),
            modified: modified.len(),
        };

        Ok(Response::new(proto::DiffSnapshotsResponse {
            from_snapshot: req.from_id,
            to_snapshot: req.to_id,
            added: added.into_iter().map(Into::into).collect(),
            removed: removed.into_iter().map(Into::into).collect(),
            modified: modified.into_iter().map(Into::into).collect(),
            summary: Some(summary.into()),
        }))
    }

    async fn diff_summarize(
        &self,
        request: Request<proto::DiffSummarizeRequest>,
    ) -> Result<Response<proto::DiffSummarizeResponse>, Status> {
        let req = request.into_inner();
        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;

        let filters = SymbolFilters {
            kind: if req.kind.is_empty() {
                None
            } else {
                Some(req.kind)
            },
            include_private: req.include_private,
            ..Default::default()
        };

        let from_symbols = self
            .db
            .list_symbols_for_snapshot(req.from_id, &filters)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let to_symbols = self
            .db
            .list_symbols_for_snapshot(req.to_id, &filters)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

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
                added_lines.push(format!(
                    "+ {} {} ({})",
                    sym.kind, sym.qualified_name, sym.signature
                ));
            }
        }
        for sym in &from_symbols {
            if !to_map.contains_key(sym.qualified_name.as_str()) {
                removed_lines.push(format!(
                    "- {} {} ({})",
                    sym.kind, sym.qualified_name, sym.signature
                ));
            }
        }
        for sym in &to_symbols {
            if let Some(from_sym) = from_map.get(sym.qualified_name.as_str()) {
                let mut changes = Vec::new();
                if from_sym.signature != sym.signature {
                    changes.push(format!(
                        "signature: {} -> {}",
                        from_sym.signature, sym.signature
                    ));
                }
                if from_sym.visibility != sym.visibility {
                    changes.push(format!(
                        "visibility: {} -> {}",
                        from_sym.visibility, sym.visibility
                    ));
                }
                if !changes.is_empty() {
                    modified_lines.push(format!(
                        "~ {} {}: {}",
                        sym.kind,
                        sym.qualified_name,
                        changes.join(", ")
                    ));
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
            for l in &added_lines {
                diff_text.push_str(l);
                diff_text.push('\n');
            }
        }
        if !removed_lines.is_empty() {
            diff_text.push_str("\nREMOVED:\n");
            for l in &removed_lines {
                diff_text.push_str(l);
                diff_text.push('\n');
            }
        }
        if !modified_lines.is_empty() {
            diff_text.push_str("\nMODIFIED:\n");
            for l in &modified_lines {
                diff_text.push_str(l);
                diff_text.push('\n');
            }
        }

        if diff_text.is_empty() {
            return Ok(Response::new(proto::DiffSummarizeResponse {
                from_snapshot: req.from_id,
                to_snapshot: req.to_id,
                changelog: "No changes detected between these snapshots.".to_string(),
                stats: Some(stats.into()),
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
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::DiffSummarizeResponse {
            from_snapshot: req.from_id,
            to_snapshot: req.to_id,
            changelog: response.content,
            stats: Some(stats.into()),
        }))
    }

    async fn delete_snapshot(
        &self,
        request: Request<proto::DeleteSnapshotRequest>,
    ) -> Result<Response<proto::DeleteSnapshotResponse>, Status> {
        let snapshot_id = request.into_inner().snapshot_id;
        let existed = self
            .db
            .delete_snapshot(snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !existed {
            return Err(Status::not_found("snapshot not found"));
        }
        let _ = self.db.gc_orphans().await;
        Ok(Response::new(proto::DeleteSnapshotResponse {
            deleted: true,
        }))
    }
}
