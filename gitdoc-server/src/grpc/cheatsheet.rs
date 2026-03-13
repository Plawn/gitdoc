use r2e::prelude::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::proto;
use crate::AppState;

#[derive(Controller)]
#[controller(state = AppState)]
pub struct CheatsheetGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[grpc_routes(proto::cheatsheet_service_server::CheatsheetService)]
impl CheatsheetGrpcService {
    async fn get_cheatsheet(
        &self,
        request: Request<proto::GetCheatsheetRequest>,
    ) -> Result<Response<proto::GetCheatsheetResponse>, Status> {
        let repo_id = request.into_inner().repo_id;
        let cheatsheet = self
            .db
            .get_cheatsheet(&repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("cheatsheet not found"))?;
        Ok(Response::new(cheatsheet.into()))
    }

    async fn generate_cheatsheet(
        &self,
        request: Request<proto::GenerateCheatsheetRequest>,
    ) -> Result<Response<proto::GenerateCheatsheetResponse>, Status> {
        let req = request.into_inner();
        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;

        let trigger = if req.trigger.is_empty() {
            "manual"
        } else {
            &req.trigger
        };

        let snapshot_id = if req.snapshot_id == 0 {
            None
        } else {
            Some(req.snapshot_id)
        };

        let patch_id = crate::cheatsheet::generate_and_store_cheatsheet(
            llm_client.clone(),
            &self.db,
            &req.repo_id,
            snapshot_id,
            trigger,
            None,
            self.embedder.as_deref(),
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let cs = self
            .db
            .get_cheatsheet(&req.repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("cheatsheet vanished after creation"))?;

        Ok(Response::new(proto::GenerateCheatsheetResponse {
            repo_id: cs.repo_id,
            patch_id,
            content: cs.content,
            model: cs.model,
            updated_at: cs.updated_at.to_rfc3339(),
        }))
    }

    async fn list_patches(
        &self,
        request: Request<proto::ListPatchesRequest>,
    ) -> Result<Response<proto::ListPatchesResponse>, Status> {
        let req = request.into_inner();
        let limit = if req.limit == 0 { 20 } else { req.limit };
        let offset = req.offset;

        let patches = self
            .db
            .list_cheatsheet_patches(&req.repo_id, limit, offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::ListPatchesResponse {
            patches: patches.into_iter().map(Into::into).collect(),
            total: 0, // list_cheatsheet_patches doesn't return total
        }))
    }

    async fn get_patch(
        &self,
        request: Request<proto::GetPatchRequest>,
    ) -> Result<Response<proto::GetPatchResponse>, Status> {
        let req = request.into_inner();
        let patch = self
            .db
            .get_cheatsheet_patch(&req.repo_id, req.patch_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("patch not found"))?;
        Ok(Response::new(proto::GetPatchResponse {
            patch: Some(patch.into()),
        }))
    }
}
