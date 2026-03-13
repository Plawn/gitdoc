use r2e::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gitdoc_api_types::requests::PublicApiQuery;
use gitdoc_api_types::responses::{PublicApiEntry, PublicApiMethod, PublicApiResponse};

use crate::AppState;
use crate::error::GitdocError;
use crate::util::path_to_module;

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct PublicApiController {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[routes]
impl PublicApiController {
    #[get("/{snapshot_id}/public_api")]
    async fn get_public_api(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<PublicApiQuery>,
    ) -> Result<Json<PublicApiResponse>, GitdocError> {
        let limit = q.limit.unwrap_or(2000);
        let offset = q.offset.unwrap_or(0);

        let symbols = self
            .db
            .get_public_api_symbols(snapshot_id, q.module_path.as_deref(), limit, offset)
            .await?;

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

        let total_items = entries.len();
        let mut modules: BTreeMap<String, Vec<PublicApiEntry>> = BTreeMap::new();
        for entry in entries {
            let module = path_to_module(&entry.file_path);
            modules.entry(module).or_default().push(entry);
        }

        Ok(Json(PublicApiResponse {
            snapshot_id,
            module_path: q.module_path,
            modules,
            total_items,
        }))
    }
}
