use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;
use crate::db::SymbolFilters;
use crate::error::GitdocError;

pub use crate::util::path_to_module;

#[derive(Serialize)]
pub struct OverviewSymbol {
    id: i64,
    name: String,
    qualified_name: String,
    kind: String,
    visibility: String,
    file_path: String,
    signature: String,
    doc_comment: Option<String>,
}

#[derive(Serialize)]
pub struct OverviewResponse {
    pub snapshot: crate::db::SnapshotRow,
    pub readme: Option<String>,
    pub docs: Vec<crate::db::DocRow>,
    pub top_level_symbols: Vec<OverviewSymbol>,
}

#[derive(Serialize)]
pub struct DiffSymbolEntry {
    name: String,
    qualified_name: String,
    kind: String,
    visibility: String,
    file_path: String,
    signature: String,
}

#[derive(Serialize)]
pub struct DiffModifiedEntry {
    qualified_name: String,
    kind: String,
    changes: Vec<String>,
    from: DiffSigVis,
    to: DiffSigVis,
}

#[derive(Serialize)]
pub struct DiffSigVis {
    signature: String,
    visibility: String,
}

#[derive(Serialize)]
pub struct DiffSummary {
    added: usize,
    removed: usize,
    modified: usize,
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub from_snapshot: i64,
    pub to_snapshot: i64,
    pub added: Vec<DiffSymbolEntry>,
    pub removed: Vec<DiffSymbolEntry>,
    pub modified: Vec<DiffModifiedEntry>,
    pub summary: DiffSummary,
}

#[derive(Serialize)]
pub struct DeleteSnapshotResponse {
    pub deleted: bool,
    pub gc: crate::db::GcStats,
}

pub async fn get_overview(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<OverviewResponse>, GitdocError> {
    let snapshot = state.db.get_snapshot(snapshot_id).await?
        .ok_or_else(|| GitdocError::NotFound("snapshot not found".into()))?;

    let docs = state.db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_else(|e| {
        tracing::warn!(snapshot_id, error = %e, "failed to list docs for snapshot");
        Vec::new()
    });
    let readme = docs.iter().find(|d| {
        let lower = d.file_path.to_lowercase();
        lower == "readme.md" || lower.ends_with("/readme.md")
    });
    let readme_content = if let Some(r) = readme {
        match state.db.get_doc_content(snapshot_id, &r.file_path).await {
            Ok(dc) => dc.and_then(|d| d.content),
            Err(e) => {
                tracing::warn!(snapshot_id, file_path = %r.file_path, error = %e, "failed to load readme content");
                None
            }
        }
    } else {
        None
    };

    let symbols = state
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

pub async fn list_docs(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<Vec<crate::db::DocRow>>, GitdocError> {
    let docs = state.db.list_docs_for_snapshot(snapshot_id).await?;
    Ok(Json(docs))
}

pub async fn get_doc_content(
    State(state): State<Arc<AppState>>,
    Path((snapshot_id, path)): Path<(i64, String)>,
) -> Result<Json<crate::db::DocContent>, GitdocError> {
    let doc = state.db.get_doc_content(snapshot_id, &path).await?
        .ok_or_else(|| GitdocError::NotFound("doc not found".into()))?;
    Ok(Json(doc))
}

use gitdoc_api_types::requests::DiffQuery;

pub async fn diff_symbols(
    State(state): State<Arc<AppState>>,
    Path((from_id, to_id)): Path<(i64, i64)>,
    Query(q): Query<DiffQuery>,
) -> Result<Json<DiffResponse>, GitdocError> {
    let include_private = q.include_private.unwrap_or(false);
    let filters = SymbolFilters {
        kind: q.kind.clone(),
        include_private,
        ..Default::default()
    };

    let from_symbols = state.db.list_symbols_for_snapshot(from_id, &filters).await?;
    let to_symbols = state.db.list_symbols_for_snapshot(to_id, &filters).await?;

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
                modified.push(DiffModifiedEntry {
                    qualified_name: sym.qualified_name.clone(),
                    kind: sym.kind.clone(),
                    changes,
                    from: DiffSigVis {
                        signature: from_sym.signature.clone(),
                        visibility: from_sym.visibility.clone(),
                    },
                    to: DiffSigVis {
                        signature: sym.signature.clone(),
                        visibility: sym.visibility.clone(),
                    },
                });
            }
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

pub async fn delete_snapshot(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
) -> Result<Json<DeleteSnapshotResponse>, GitdocError> {
    let existed = state.db.delete_snapshot(snapshot_id).await?;
    if !existed {
        return Err(GitdocError::NotFound("snapshot not found".into()));
    }
    let gc = state.db.gc_orphans().await?;
    Ok(Json(DeleteSnapshotResponse { deleted: true, gc }))
}
