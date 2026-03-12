use r2e::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gitdoc_api_types::requests::ModuleTreeQuery;

use crate::AppState;
use crate::error::GitdocError;
use crate::util::path_to_module;

#[derive(Serialize)]
pub struct ModuleTreeNode {
    name: String,
    path: String,
    doc_comment: Option<String>,
    public_items: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<ModuleTreeNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    symbols: Vec<ModuleTreeSymbol>,
}

#[derive(Serialize, Clone)]
pub struct ModuleTreeSymbol {
    name: String,
    kind: String,
    signature: String,
}

#[derive(Serialize)]
pub struct ModuleTreeResponse {
    snapshot_id: i64,
    tree: Vec<ModuleTreeNode>,
}

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct ModuleTreeController {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[routes]
impl ModuleTreeController {
    #[get("/{snapshot_id}/module_tree")]
    async fn get_module_tree(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<ModuleTreeQuery>,
    ) -> Result<Json<ModuleTreeResponse>, GitdocError> {
        let max_depth = q.depth.unwrap_or(usize::MAX);
        let include_sigs = q.include_signatures.unwrap_or(false);

        let (file_infos, module_syms) = tokio::join!(
            self.db.get_snapshot_file_paths(snapshot_id),
            self.db.get_module_symbols(snapshot_id),
        );
        let file_infos = file_infos?;
        let module_syms = module_syms?;

        let mut module_docs: HashMap<String, String> = HashMap::new();
        for m in &module_syms {
            if let Some(ref doc) = m.doc_comment {
                let mod_path = path_to_module(&m.file_path);
                module_docs.insert(mod_path, doc.clone());
            }
        }

        let mut module_info: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
        for fi in &file_infos {
            if fi.file_type == "other" {
                continue;
            }
            let mod_path = path_to_module(&fi.file_path);
            let entry = module_info.entry(mod_path).or_insert((0, Vec::new()));
            entry.0 += fi.public_symbol_count;
            entry.1.push(fi.file_path.clone());
        }

        let sig_map: HashMap<String, Vec<ModuleTreeSymbol>> = if include_sigs {
            let all_paths: Vec<String> = file_infos.iter().map(|f| f.file_path.clone()).collect();
            let syms = self.db.get_public_signatures_by_file(snapshot_id, &all_paths).await?;
            let mut m: HashMap<String, Vec<ModuleTreeSymbol>> = HashMap::new();
            for s in syms {
                if s.kind == "impl" {
                    continue;
                }
                let mod_path = path_to_module(&s.file_path);
                m.entry(mod_path).or_default().push(ModuleTreeSymbol {
                    name: s.name,
                    kind: s.kind,
                    signature: s.signature,
                });
            }
            m
        } else {
            HashMap::new()
        };

        let root = build_module_tree(&module_info, &module_docs, &sig_map, max_depth);

        Ok(Json(ModuleTreeResponse {
            snapshot_id,
            tree: root,
        }))
    }
}

fn build_module_tree(
    module_info: &BTreeMap<String, (i64, Vec<String>)>,
    module_docs: &HashMap<String, String>,
    sig_map: &HashMap<String, Vec<ModuleTreeSymbol>>,
    max_depth: usize,
) -> Vec<ModuleTreeNode> {
    let mut root_children: BTreeMap<String, ModuleTreeBuilder> = BTreeMap::new();

    for (path, (count, _)) in module_info {
        let parts: Vec<&str> = if path == "crate" {
            vec!["crate"]
        } else {
            path.split("::").collect()
        };

        if parts.is_empty() {
            continue;
        }

        let mut current = &mut root_children;
        for (i, part) in parts.iter().enumerate() {
            let partial_path = parts[..=i].join("::");
            let node = current.entry(part.to_string()).or_insert_with(|| ModuleTreeBuilder {
                path: partial_path.clone(),
                doc_comment: module_docs.get(&partial_path).cloned(),
                public_items: 0,
                children: BTreeMap::new(),
                symbols: Vec::new(),
            });

            if i == parts.len() - 1 {
                node.public_items += count;
                if let Some(sigs) = sig_map.get(&partial_path) {
                    node.symbols = sigs.iter().map(|s| ModuleTreeSymbol {
                        name: s.name.clone(),
                        kind: s.kind.clone(),
                        signature: s.signature.clone(),
                    }).collect();
                }
            }
            current = &mut node.children;
        }
    }

    fn convert(map: &BTreeMap<String, ModuleTreeBuilder>, depth: usize, max_depth: usize) -> Vec<ModuleTreeNode> {
        map.iter().map(|(name, builder)| {
            ModuleTreeNode {
                name: name.clone(),
                path: builder.path.clone(),
                doc_comment: builder.doc_comment.clone(),
                public_items: builder.public_items,
                children: if depth < max_depth {
                    convert(&builder.children, depth + 1, max_depth)
                } else {
                    Vec::new()
                },
                symbols: builder.symbols.clone(),
            }
        }).collect()
    }

    convert(&root_children, 0, max_depth)
}

struct ModuleTreeBuilder {
    path: String,
    doc_comment: Option<String>,
    public_items: i64,
    children: BTreeMap<String, ModuleTreeBuilder>,
    symbols: Vec<ModuleTreeSymbol>,
}
