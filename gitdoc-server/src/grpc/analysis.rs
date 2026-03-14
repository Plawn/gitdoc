use r2e::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use gitdoc_api_types::responses::{
    MethodInfo, ModuleTreeNode, ModuleTreeSymbol,
    PublicApiEntry, PublicApiMethod, RelevantDoc, RelevantSymbol,
};

use super::proto;
use crate::AppState;
use crate::embeddings;
use crate::util::path_to_module;

#[derive(Controller)]
#[controller(state = AppState)]
pub struct AnalysisGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[grpc_routes(proto::analysis_service_server::AnalysisService)]
impl AnalysisGrpcService {
    async fn get_public_api(
        &self,
        request: Request<proto::GetPublicApiRequest>,
    ) -> Result<Response<proto::GetPublicApiResponse>, Status> {
        let req = request.into_inner();
        let module_path = if req.module_path.is_empty() {
            None
        } else {
            Some(req.module_path.clone())
        };
        let limit = if req.limit == 0 { 2000 } else { req.limit };
        let offset = if req.offset == 0 { 0 } else { req.offset };

        let symbols = self
            .db
            .get_public_api_symbols(req.snapshot_id, module_path.as_deref(), limit, offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

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

        let proto_modules: HashMap<String, proto::PublicApiModuleEntries> = modules
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    proto::PublicApiModuleEntries {
                        entries: v.into_iter().map(Into::into).collect(),
                    },
                )
            })
            .collect();

        Ok(Response::new(proto::GetPublicApiResponse {
            snapshot_id: req.snapshot_id,
            module_path: module_path.unwrap_or_default(),
            modules: proto_modules,
            total_items: total_items as u64,
        }))
    }

    async fn get_module_tree(
        &self,
        request: Request<proto::GetModuleTreeRequest>,
    ) -> Result<Response<proto::GetModuleTreeResponse>, Status> {
        let req = request.into_inner();
        let max_depth = if req.depth == 0 {
            usize::MAX
        } else {
            req.depth as usize
        };

        let (file_infos, module_syms) = tokio::join!(
            self.db.get_snapshot_file_paths(req.snapshot_id),
            self.db.get_module_symbols(req.snapshot_id),
        );
        let file_infos = file_infos.map_err(|e| Status::internal(e.to_string()))?;
        let module_syms = module_syms.map_err(|e| Status::internal(e.to_string()))?;

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

        let sig_map: HashMap<String, Vec<ModuleTreeSymbol>> = if req.include_signatures {
            let all_paths: Vec<String> =
                file_infos.iter().map(|f| f.file_path.clone()).collect();
            let syms = self
                .db
                .get_public_signatures_by_file(req.snapshot_id, &all_paths)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
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

        let tree = build_module_tree(&module_info, &module_docs, &sig_map, max_depth);

        Ok(Response::new(proto::GetModuleTreeResponse {
            snapshot_id: req.snapshot_id,
            tree: tree.into_iter().map(Into::into).collect(),
        }))
    }

    async fn explain(
        &self,
        request: Request<proto::ExplainRequest>,
    ) -> Result<Response<proto::ExplainResponse>, Status> {
        let req = request.into_inner();
        if req.q.is_empty() {
            return Err(Status::invalid_argument("q must be non-empty"));
        }

        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| Status::unavailable("no embedding provider configured"))?;

        let limit = if req.limit == 0 { 10 } else { req.limit as usize };

        let query_vec = embedder
            .embed_query(&req.q)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let file_ids = self
            .db
            .get_file_ids_for_snapshot(req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let query_pgvec = embeddings::to_pgvector(&query_vec);

        let search_results = self
            .db
            .search_embeddings_by_vector(&query_pgvec, &file_ids, "all", limit as i64)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let docs = self
            .db
            .list_docs_for_snapshot(req.snapshot_id)
            .await
            .unwrap_or_default();

        let mut relevant_symbols: Vec<RelevantSymbol> = Vec::new();
        let mut relevant_docs: Vec<RelevantDoc> = Vec::new();
        let mut seen_symbols: HashSet<i64> = HashSet::new();

        for r in &search_results {
            match r.source_type.as_str() {
                "symbol" => {
                    if seen_symbols.contains(&r.source_id) {
                        continue;
                    }
                    seen_symbols.insert(r.source_id);

                    if let Ok(Some(sym)) = self.db.get_symbol_by_id(r.source_id).await {
                        let methods = if matches!(
                            sym.kind.as_str(),
                            "struct" | "enum" | "trait" | "class" | "interface"
                        ) {
                            let children = self
                                .db
                                .list_symbol_children(sym.id)
                                .await
                                .unwrap_or_default();
                            children
                                .iter()
                                .filter(|c| c.kind == "function")
                                .map(|c| MethodInfo {
                                    name: c.name.clone(),
                                    signature: c.signature.clone(),
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };

                        let traits = if matches!(
                            sym.kind.as_str(),
                            "struct" | "enum" | "class"
                        ) {
                            let impls = self
                                .db
                                .get_implementations(sym.id, req.snapshot_id)
                                .await
                                .unwrap_or_default();
                            impls
                                .iter()
                                .filter(|i| {
                                    i.symbol.kind == "trait" || i.symbol.kind == "interface"
                                })
                                .map(|i| i.symbol.qualified_name.clone())
                                .collect()
                        } else {
                            Vec::new()
                        };

                        relevant_symbols.push(RelevantSymbol {
                            id: sym.id,
                            name: sym.name,
                            qualified_name: sym.qualified_name,
                            kind: sym.kind,
                            signature: sym.signature,
                            doc_comment: sym.doc_comment,
                            file_path: sym.file_path,
                            score: r.score,
                            methods,
                            traits,
                        });
                    }
                }
                "doc_chunk" => {
                    if let Some(doc) = docs.iter().find(|d| d.id == r.source_id) {
                        relevant_docs.push(RelevantDoc {
                            file_path: doc.file_path.clone(),
                            title: doc.title.clone(),
                            snippet: r.text.clone(),
                            score: r.score,
                        });
                    }
                }
                _ => {}
            }
        }

        let synthesis = if req.synthesize {
            if let Some(ref llm_client) = self.llm_client {
                Some(
                    crate::llm::synthesize_answer(llm_client, &req.q, &relevant_symbols, &relevant_docs)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?,
                )
            } else {
                Some(
                    "LLM synthesis unavailable — no LLM provider configured (set GITDOC_LLM_ENDPOINT)"
                        .into(),
                )
            }
        } else {
            None
        };

        Ok(Response::new(proto::ExplainResponse {
            query: req.q,
            relevant_symbols: relevant_symbols.into_iter().map(Into::into).collect(),
            relevant_docs: relevant_docs.into_iter().map(Into::into).collect(),
            synthesis: synthesis.unwrap_or_default(),
        }))
    }

    async fn summarize(
        &self,
        request: Request<proto::SummarizeRequest>,
    ) -> Result<Response<proto::SummarizeResponse>, Status> {
        let req = request.into_inner();
        if req.scope.is_empty() {
            return Err(Status::invalid_argument("scope is required"));
        }

        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;

        let content = crate::llm::generate_and_store_summary(
            llm_client.clone(),
            &self.db,
            req.snapshot_id,
            &req.scope,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::SummarizeResponse {
            snapshot_id: req.snapshot_id,
            scope: req.scope,
            content,
        }))
    }

    async fn get_summary(
        &self,
        request: Request<proto::GetSummaryRequest>,
    ) -> Result<Response<proto::GetSummaryResponse>, Status> {
        let req = request.into_inner();

        let summaries = if req.scope.is_empty() {
            self.db
                .list_summaries(req.snapshot_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
        } else {
            match self
                .db
                .get_summary(req.snapshot_id, &req.scope)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
            {
                Some(s) => vec![s],
                None => Vec::new(),
            }
        };

        Ok(Response::new(proto::GetSummaryResponse {
            summaries: summaries.into_iter().map(Into::into).collect(),
        }))
    }
}

// Module tree builder (inlined from api/module_tree.rs)

struct ModuleTreeBuilder {
    path: String,
    doc_comment: Option<String>,
    public_items: i64,
    children: BTreeMap<String, ModuleTreeBuilder>,
    symbols: Vec<ModuleTreeSymbol>,
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
            let node = current
                .entry(part.to_string())
                .or_insert_with(|| ModuleTreeBuilder {
                    path: partial_path.clone(),
                    doc_comment: module_docs.get(&partial_path).cloned(),
                    public_items: 0,
                    children: BTreeMap::new(),
                    symbols: Vec::new(),
                });

            if i == parts.len() - 1 {
                node.public_items += count;
                if let Some(sigs) = sig_map.get(&partial_path) {
                    node.symbols = sigs
                        .iter()
                        .map(|s| ModuleTreeSymbol {
                            name: s.name.clone(),
                            kind: s.kind.clone(),
                            signature: s.signature.clone(),
                        })
                        .collect();
                }
            }
            current = &mut node.children;
        }
    }

    fn convert(
        map: &BTreeMap<String, ModuleTreeBuilder>,
        depth: usize,
        max_depth: usize,
    ) -> Vec<ModuleTreeNode> {
        map.iter()
            .map(|(name, builder)| ModuleTreeNode {
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
            })
            .collect()
    }

    convert(&root_children, 0, max_depth)
}

