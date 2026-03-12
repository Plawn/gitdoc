use r2e::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;
use crate::db::{RefWithSymbol, SymbolDetail, SymbolRow};
use crate::error::GitdocError;

#[derive(Serialize)]
pub struct TypeContextResponse {
    pub symbol: SymbolDetail,
    pub methods: Vec<SymbolRow>,
    pub fields: Vec<SymbolRow>,
    pub traits_implemented: Vec<RefWithSymbol>,
    pub implementors: Vec<RefWithSymbol>,
    pub used_by: UsedBy,
    pub depends_on: DependsOn,
}

#[derive(Serialize)]
pub struct UsedBy {
    pub callers: Vec<RefWithSymbol>,
    pub type_users: Vec<RefWithSymbol>,
}

#[derive(Serialize)]
pub struct DependsOn {
    pub types: Vec<RefWithSymbol>,
    pub calls: Vec<RefWithSymbol>,
}

#[derive(Serialize)]
pub struct ExamplesResponse {
    pub symbol_id: i64,
    pub symbol_name: String,
    pub examples: Vec<crate::indexer::doc_parser::CodeExample>,
}

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct TypeContextController {
    #[inject]
    db: Arc<crate::db::Database>,
}

#[routes]
impl TypeContextController {
    #[get("/{snapshot_id}/symbols/{symbol_id}/type_context")]
    async fn get_type_context(
        &self,
        Path((snapshot_id, symbol_id)): Path<(i64, i64)>,
    ) -> Result<Json<TypeContextResponse>, GitdocError> {
        let (symbol_res, children_res, impls_res, inbound_res, outbound_res) = tokio::join!(
            self.db.get_symbol_by_id(symbol_id),
            self.db.list_symbol_children(symbol_id),
            self.db.get_implementations(symbol_id, snapshot_id),
            self.db.get_inbound_refs(symbol_id, snapshot_id, None, 50),
            self.db.get_outbound_refs(symbol_id, snapshot_id, None, 50),
        );

        let symbol = symbol_res?
            .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;
        let children = children_res.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, error = %e, "failed to list symbol children");
            Vec::new()
        });
        let implementations = impls_res.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, snapshot_id, error = %e, "failed to get implementations");
            Vec::new()
        });
        let inbound = inbound_res.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, snapshot_id, error = %e, "failed to get inbound refs");
            Vec::new()
        });
        let outbound = outbound_res.unwrap_or_else(|e| {
            tracing::warn!(symbol_id, snapshot_id, error = %e, "failed to get outbound refs");
            Vec::new()
        });

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

        Ok(Json(TypeContextResponse {
            symbol,
            methods,
            fields,
            traits_implemented,
            implementors,
            used_by: UsedBy { callers, type_users },
            depends_on: DependsOn { types: dep_types, calls: dep_calls },
        }))
    }

    #[get("/{snapshot_id}/symbols/{symbol_id}/examples")]
    async fn get_examples(
        &self,
        Path((_snapshot_id, symbol_id)): Path<(i64, i64)>,
    ) -> Result<Json<ExamplesResponse>, GitdocError> {
        let symbol = self.db.get_symbol_by_id(symbol_id).await?
            .ok_or_else(|| GitdocError::NotFound("symbol not found".into()))?;

        let examples = if let Some(ref doc) = symbol.doc_comment {
            crate::indexer::doc_parser::extract_code_examples(doc)
        } else {
            Vec::new()
        };

        Ok(Json(ExamplesResponse {
            symbol_id,
            symbol_name: symbol.name,
            examples,
        }))
    }
}
