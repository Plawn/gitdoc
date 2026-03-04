use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;
use crate::util::path_to_module;

#[derive(Deserialize)]
pub struct PublicApiQuery {
    pub module_path: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
struct PublicApiEntry {
    id: i64,
    name: String,
    qualified_name: String,
    kind: String,
    visibility: String,
    file_path: String,
    signature: String,
    doc_comment: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<PublicApiMethod>,
}

#[derive(Serialize)]
struct PublicApiMethod {
    id: i64,
    name: String,
    signature: String,
    doc_comment: Option<String>,
}

pub async fn get_public_api(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<i64>,
    Query(q): Query<PublicApiQuery>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let limit = q.limit.unwrap_or(2000);
    let offset = q.offset.unwrap_or(0);

    let symbols = state
        .db
        .get_public_api_symbols(snapshot_id, q.module_path.as_deref(), limit, offset)
        .await?;

    // First pass: separate impl blocks from other symbols
    let mut type_map: HashMap<(String, String), usize> = HashMap::new();
    let mut entries: Vec<PublicApiEntry> = Vec::new();
    let mut impl_children: Vec<(String, String, PublicApiMethod)> = Vec::new();

    for sym in &symbols {
        if sym.kind == "impl" {
            continue;
        }

        if let Some(parent_id) = sym.parent_id {
            if let Some(parent) = symbols.iter().find(|s| s.id == parent_id) {
                if parent.kind == "impl" {
                    impl_children.push((
                        parent.name.clone(),
                        parent.file_path.clone(),
                        PublicApiMethod {
                            id: sym.id,
                            name: sym.name.clone(),
                            signature: sym.signature.clone(),
                            doc_comment: sym.doc_comment.clone(),
                        },
                    ));
                    continue;
                }
            }
        }

        let key = (sym.name.clone(), sym.file_path.clone());
        let idx = entries.len();
        type_map.insert(key, idx);
        entries.push(PublicApiEntry {
            id: sym.id,
            name: sym.name.clone(),
            qualified_name: sym.qualified_name.clone(),
            kind: sym.kind.clone(),
            visibility: sym.visibility.clone(),
            file_path: sym.file_path.clone(),
            signature: sym.signature.clone(),
            doc_comment: sym.doc_comment.clone(),
            methods: Vec::new(),
        });
    }

    // Second pass: merge impl children onto their parent types
    for (parent_name, file_path, method) in impl_children {
        if let Some(&idx) = type_map.get(&(parent_name.clone(), file_path.clone())) {
            entries[idx].methods.push(method);
        } else {
            let key = (parent_name.clone(), file_path.clone());
            let idx = entries.len();
            type_map.insert(key, idx);
            entries.push(PublicApiEntry {
                id: 0,
                name: parent_name,
                qualified_name: String::new(),
                kind: "type".into(),
                visibility: "pub".into(),
                file_path,
                signature: String::new(),
                doc_comment: None,
                methods: vec![method],
            });
        }
    }

    // Group by module
    let mut modules: BTreeMap<String, Vec<&PublicApiEntry>> = BTreeMap::new();
    for entry in &entries {
        let module = path_to_module(&entry.file_path);
        modules.entry(module).or_default().push(entry);
    }

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id,
        "module_path": q.module_path,
        "modules": modules,
        "total_items": entries.len(),
    })))
}
