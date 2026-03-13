use r2e::prelude::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::proto;
use crate::db::SymbolFilters;
use crate::AppState;

#[derive(Controller)]
#[controller(state = AppState)]
pub struct SymbolGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[grpc_routes(proto::symbol_service_server::SymbolService)]
impl SymbolGrpcService {
    async fn list_symbols(
        &self,
        request: Request<proto::ListSymbolsRequest>,
    ) -> Result<Response<proto::ListSymbolsResponse>, Status> {
        let req = request.into_inner();
        let filters = SymbolFilters {
            kind: if req.kind.is_empty() { None } else { Some(req.kind) },
            visibility: if req.visibility.is_empty() { None } else { Some(req.visibility) },
            file_path: if req.file_path.is_empty() { None } else { Some(req.file_path) },
            include_private: req.include_private,
        };
        let symbols = self
            .db
            .list_symbols_for_snapshot(req.snapshot_id, &filters)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListSymbolsResponse {
            symbols: symbols.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_snapshot_symbol(
        &self,
        request: Request<proto::GetSnapshotSymbolRequest>,
    ) -> Result<Response<proto::GetSnapshotSymbolResponse>, Status> {
        let req = request.into_inner();
        let detail = self
            .db
            .get_symbol_by_id(req.symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("symbol not found"))?;
        let children = self
            .db
            .list_symbol_children(req.symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let (ref_by_count, ref_count) = self
            .db
            .count_refs_for_symbol(req.symbol_id, req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::GetSnapshotSymbolResponse {
            symbol: Some(detail.into()),
            children: children.into_iter().map(Into::into).collect(),
            referenced_by_count: ref_by_count,
            references_count: ref_count,
        }))
    }

    async fn get_symbol(
        &self,
        request: Request<proto::GetSymbolRequest>,
    ) -> Result<Response<proto::GetSymbolResponse>, Status> {
        let symbol_id = request.into_inner().symbol_id;
        let detail = self
            .db
            .get_symbol_by_id(symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("symbol not found"))?;
        let children = self
            .db
            .list_symbol_children(symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::GetSymbolResponse {
            symbol: Some(detail.into()),
            children: children.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_symbol_references(
        &self,
        request: Request<proto::GetSymbolReferencesRequest>,
    ) -> Result<Response<proto::GetSymbolReferencesResponse>, Status> {
        let req = request.into_inner();
        let direction = if req.direction.is_empty() {
            "inbound"
        } else {
            &req.direction
        };
        let kind = if req.kind.is_empty() { None } else { Some(req.kind.as_str()) };
        let limit = if req.limit == 0 { 100 } else { req.limit };

        let refs = match direction {
            "outbound" => self
                .db
                .get_outbound_refs(req.symbol_id, req.snapshot_id, kind, limit)
                .await
                .map_err(|e| Status::internal(e.to_string()))?,
            _ => self
                .db
                .get_inbound_refs(req.symbol_id, req.snapshot_id, kind, limit)
                .await
                .map_err(|e| Status::internal(e.to_string()))?,
        };
        Ok(Response::new(proto::GetSymbolReferencesResponse {
            refs: refs.into_iter().map(Into::into).collect(),
        }))
    }

    async fn batch_get_symbols(
        &self,
        request: Request<proto::BatchGetSymbolsRequest>,
    ) -> Result<Response<proto::BatchGetSymbolsResponse>, Status> {
        let req = request.into_inner();
        if req.ids.len() > 100 {
            return Err(Status::invalid_argument("max 100 ids"));
        }
        let symbols = self
            .db
            .get_symbols_by_ids(&req.ids)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::BatchGetSymbolsResponse {
            symbols: symbols.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_symbol_context(
        &self,
        request: Request<proto::GetSymbolContextRequest>,
    ) -> Result<Response<proto::GetSymbolContextResponse>, Status> {
        let req = request.into_inner();
        let detail = self
            .db
            .get_symbol_by_id(req.symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("symbol not found"))?;
        let children = self
            .db
            .list_symbol_children(req.symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let (ref_by_count, ref_count) = self
            .db
            .count_refs_for_symbol(req.symbol_id, req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let include = if req.include.is_empty() {
            None
        } else {
            Some(req.include.as_str())
        };

        let mut callers = Vec::new();
        let mut callees = Vec::new();
        let mut implementations = Vec::new();

        if include == Some("all") || include == Some("callers") {
            callers = self
                .db
                .get_inbound_refs(req.symbol_id, req.snapshot_id, Some("calls"), 50)
                .await
                .unwrap_or_default();
        }
        if include == Some("all") || include == Some("callees") {
            callees = self
                .db
                .get_outbound_refs(req.symbol_id, req.snapshot_id, Some("calls"), 50)
                .await
                .unwrap_or_default();
        }
        if include == Some("all") || include == Some("implementations") {
            implementations = self
                .db
                .get_implementations(req.symbol_id, req.snapshot_id)
                .await
                .unwrap_or_default();
        }

        Ok(Response::new(proto::GetSymbolContextResponse {
            symbol: Some(detail.into()),
            children: children.into_iter().map(Into::into).collect(),
            referenced_by_count: ref_by_count,
            references_count: ref_count,
            callers: callers.into_iter().map(Into::into).collect(),
            callees: callees.into_iter().map(Into::into).collect(),
            implementations: implementations.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_implementations(
        &self,
        request: Request<proto::GetImplementationsRequest>,
    ) -> Result<Response<proto::GetImplementationsResponse>, Status> {
        let req = request.into_inner();
        let impls = self
            .db
            .get_implementations(req.symbol_id, req.snapshot_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::GetImplementationsResponse {
            implementations: impls.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_type_context(
        &self,
        request: Request<proto::GetTypeContextRequest>,
    ) -> Result<Response<proto::GetTypeContextResponse>, Status> {
        let req = request.into_inner();

        let (symbol_res, children_res, impls_res, inbound_res, outbound_res) = tokio::join!(
            self.db.get_symbol_by_id(req.symbol_id),
            self.db.list_symbol_children(req.symbol_id),
            self.db.get_implementations(req.symbol_id, req.snapshot_id),
            self.db.get_inbound_refs(req.symbol_id, req.snapshot_id, None, 50),
            self.db.get_outbound_refs(req.symbol_id, req.snapshot_id, None, 50),
        );

        let detail = symbol_res
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("symbol not found"))?;
        let children = children_res.unwrap_or_default();
        let implementations = impls_res.unwrap_or_default();
        let inbound = inbound_res.unwrap_or_default();
        let outbound = outbound_res.unwrap_or_default();

        let mut methods = Vec::new();
        let mut fields = Vec::new();
        for c in children {
            if c.kind == "function" {
                methods.push(c);
            } else {
                fields.push(c);
            }
        }

        let mut traits_implemented = Vec::new();
        let mut implementors = Vec::new();
        for r in implementations {
            if r.symbol.kind == "trait" || r.symbol.kind == "interface" {
                traits_implemented.push(r);
            } else {
                implementors.push(r);
            }
        }

        let mut callers = Vec::new();
        let mut type_users = Vec::new();
        for r in inbound {
            if r.ref_kind == "calls" {
                callers.push(r);
            } else if r.ref_kind == "type_ref" {
                type_users.push(r);
            }
        }

        let mut dep_types = Vec::new();
        let mut dep_calls = Vec::new();
        for r in outbound {
            if r.ref_kind == "type_ref" {
                dep_types.push(r);
            } else if r.ref_kind == "calls" {
                dep_calls.push(r);
            }
        }

        Ok(Response::new(proto::GetTypeContextResponse {
            symbol: Some(detail.into()),
            methods: methods.into_iter().map(Into::into).collect(),
            fields: fields.into_iter().map(Into::into).collect(),
            traits_implemented: traits_implemented.into_iter().map(Into::into).collect(),
            implementors: implementors.into_iter().map(Into::into).collect(),
            used_by: Some(proto::UsedBy {
                callers: callers.into_iter().map(Into::into).collect(),
                type_users: type_users.into_iter().map(Into::into).collect(),
            }),
            depends_on: Some(proto::DependsOn {
                types: dep_types.into_iter().map(Into::into).collect(),
                calls: dep_calls.into_iter().map(Into::into).collect(),
            }),
        }))
    }

    async fn get_examples(
        &self,
        request: Request<proto::GetExamplesRequest>,
    ) -> Result<Response<proto::GetExamplesResponse>, Status> {
        let req = request.into_inner();
        let detail = self
            .db
            .get_symbol_by_id(req.symbol_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("symbol not found"))?;

        let examples = if let Some(ref doc) = detail.doc_comment {
            crate::indexer::doc_parser::extract_code_examples(doc)
        } else {
            Vec::new()
        };

        Ok(Response::new(proto::GetExamplesResponse {
            symbol_id: req.symbol_id,
            symbol_name: detail.name,
            examples: examples
                .into_iter()
                .map(|e| proto::CodeExample {
                    language: e.language.unwrap_or_default(),
                    code: e.code,
                })
                .collect(),
        }))
    }
}
