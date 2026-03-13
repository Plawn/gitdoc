use r2e::prelude::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::proto;
use crate::error::GitdocError;
use crate::{git_ops, AppState};

#[derive(Controller)]
#[controller(state = AppState)]
pub struct RepoGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    search: Arc<crate::search::SearchIndex>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    config: Arc<crate::config::Config>,
}

#[grpc_routes(proto::repo_service_server::RepoService)]
impl RepoGrpcService {
    async fn create_repo(
        &self,
        request: Request<proto::CreateRepoRequest>,
    ) -> Result<Response<proto::CreateRepoResponse>, Status> {
        let req = request.into_inner();

        if req.id.is_empty() || req.name.is_empty() {
            return Err(GitdocError::BadRequest("id and name must be non-empty".into()).into());
        }
        if req.url.is_empty() {
            return Err(GitdocError::BadRequest("url must be non-empty".into()).into());
        }

        if let Some(existing) = self.db.get_repo(&req.id).await.map_err(|e| Status::internal(e.to_string()))? {
            if existing.url.as_deref() == Some(&req.url) {
                return Ok(Response::new(proto::CreateRepoResponse {
                    id: req.id,
                    already_existed: true,
                }));
            }
            return Err(GitdocError::Conflict(format!(
                "repo '{}' already exists with a different URL",
                req.id,
            ))
            .into());
        }

        let dest = git_ops::repo_clone_path(&self.config.repos_dir, &req.id);
        if let Err(e) = git_ops::clone_repo(&req.url, &dest).await {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            return Err(GitdocError::BadRequest(format!("clone failed: {e}")).into());
        }
        let repo_path = dest.to_string_lossy().into_owned();

        self.db
            .insert_repo(&req.id, &repo_path, &req.name, Some(&req.url))
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::CreateRepoResponse {
            id: req.id,
            already_existed: false,
        }))
    }

    async fn list_repos(
        &self,
        _request: Request<proto::ListReposRequest>,
    ) -> Result<Response<proto::ListReposResponse>, Status> {
        let repos = self
            .db
            .list_repos()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListReposResponse {
            repos: repos.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_repo(
        &self,
        request: Request<proto::GetRepoRequest>,
    ) -> Result<Response<proto::GetRepoResponse>, Status> {
        let repo_id = request.into_inner().repo_id;
        let repo = self
            .db
            .get_repo(&repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("repo not found"))?;
        let snapshots = self
            .db
            .list_snapshots(&repo_id)
            .await
            .unwrap_or_default();
        Ok(Response::new(proto::GetRepoResponse {
            repo: Some(repo.into()),
            snapshots: snapshots.into_iter().map(Into::into).collect(),
        }))
    }

    async fn delete_repo(
        &self,
        request: Request<proto::DeleteRepoRequest>,
    ) -> Result<Response<proto::DeleteRepoResponse>, Status> {
        let repo_id = request.into_inner().repo_id;
        let repo = self
            .db
            .get_repo(&repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let existed = self
            .db
            .delete_repo(&repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !existed {
            return Err(Status::not_found("repo not found"));
        }

        if let Some(repo) = repo {
            let _ = tokio::fs::remove_dir_all(&repo.path).await;
        }
        let _ = self.db.gc_orphans().await;

        Ok(Response::new(proto::DeleteRepoResponse { deleted: true }))
    }

    async fn index_repo(
        &self,
        request: Request<proto::IndexRepoRequest>,
    ) -> Result<Response<proto::IndexRepoResponse>, Status> {
        let req = request.into_inner();
        let repo = self
            .db
            .get_repo(&req.repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("repo not found"))?;

        if req.fetch {
            git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
                .await
                .map_err(|e| Status::invalid_argument(format!("fetch failed: {e}")))?;
        }

        let commit = if req.commit.is_empty() {
            "HEAD".to_string()
        } else {
            req.commit
        };
        let label = if req.label.is_empty() {
            None
        } else {
            Some(req.label)
        };

        let result = crate::indexer::pipeline::run_indexation(
            &self.db,
            &self.search,
            &req.repo_id,
            std::path::Path::new(&repo.path),
            &commit,
            label.as_deref(),
            self.embedder.clone(),
            &self.config.exclusion_patterns,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(result.into()))
    }

    async fn fetch_repo(
        &self,
        request: Request<proto::FetchRepoRequest>,
    ) -> Result<Response<proto::FetchRepoResponse>, Status> {
        let repo_id = request.into_inner().repo_id;
        let repo = self
            .db
            .get_repo(&repo_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("repo not found"))?;

        git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
            .await
            .map_err(|e| Status::invalid_argument(format!("fetch failed: {e}")))?;

        Ok(Response::new(proto::FetchRepoResponse {
            fetched: true,
            repo_id,
        }))
    }
}
