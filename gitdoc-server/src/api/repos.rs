use r2e::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use gitdoc_api_types::requests::CreateRepoBody;
use gitdoc_api_types::responses::{CreateRepoResponse, FetchRepoResponse};

use crate::AppState;
use crate::error::GitdocError;
use crate::git_ops;

#[derive(Serialize)]
pub struct GetRepoResponse {
    pub repo: crate::db::RepoRow,
    pub snapshots: Vec<crate::db::SnapshotRow>,
}

#[derive(Serialize)]
pub struct DeleteRepoResponse {
    pub deleted: bool,
    pub gc: crate::db::GcStats,
}

use gitdoc_api_types::requests::IndexBody;

#[derive(Controller)]
#[controller(path = "/repos", state = AppState)]
pub struct RepoController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    search: Arc<crate::search::SearchIndex>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    config: Arc<crate::config::Config>,
}

#[routes]
impl RepoController {
    #[post("/")]
    async fn create_repo(
        &self,
        Json(body): Json<CreateRepoBody>,
    ) -> Result<(StatusCode, Json<CreateRepoResponse>), GitdocError> {
        if body.id.is_empty() || body.name.is_empty() {
            return Err(GitdocError::BadRequest("id and name must be non-empty".into()));
        }
        if body.url.is_empty() {
            return Err(GitdocError::BadRequest("url must be non-empty".into()));
        }

        if let Some(existing) = self.db.get_repo(&body.id).await? {
            if existing.url.as_deref() == Some(&body.url) {
                return Ok((
                    StatusCode::OK,
                    Json(CreateRepoResponse { id: body.id, already_existed: true }),
                ));
            }
            return Err(GitdocError::Conflict(format!(
                "repo '{}' already exists with a different URL (existing: {:?}, requested: {})",
                body.id,
                existing.url,
                body.url,
            )));
        }

        let dest = git_ops::repo_clone_path(&self.config.repos_dir, &body.id);
        if let Err(e) = git_ops::clone_repo(&body.url, &dest).await {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            return Err(GitdocError::BadRequest(format!("clone failed: {e}")));
        }
        let repo_path = dest.to_string_lossy().into_owned();

        self.db
            .insert_repo(&body.id, &repo_path, &body.name, Some(&body.url))
            .await?;

        Ok((
            StatusCode::CREATED,
            Json(CreateRepoResponse { id: body.id, already_existed: false }),
        ))
    }

    #[get("/")]
    async fn list_repos(
        &self,
    ) -> Result<Json<Vec<crate::db::RepoSummaryRow>>, GitdocError> {
        let repos = self.db.list_repos().await?;
        Ok(Json(repos))
    }

    #[get("/{repo_id}")]
    async fn get_repo(
        &self,
        Path(repo_id): Path<String>,
    ) -> Result<Json<GetRepoResponse>, GitdocError> {
        let repo = self.db.get_repo(&repo_id).await?
            .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;
        let snapshots = self.db.list_snapshots(&repo_id).await.unwrap_or_else(|e| {
            tracing::warn!(repo_id = %repo_id, error = %e, "failed to list snapshots for repo");
            Vec::new()
        });
        Ok(Json(GetRepoResponse { repo, snapshots }))
    }

    #[post("/{repo_id}/index")]
    async fn index_repo(
        &self,
        Path(repo_id): Path<String>,
        Json(body): Json<IndexBody>,
    ) -> Result<Json<crate::indexer::pipeline::IndexResult>, GitdocError> {
        let repo = self.db.get_repo(&repo_id).await?
            .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

        if body.fetch {
            git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
                .await
                .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;
        }

        let db = self.db.clone();
        let search = self.search.clone();
        let embedder = self.embedder.clone();
        let exclusion_patterns = self.config.exclusion_patterns.clone();
        let repo_path = repo.path.clone();
        let commit = body.commit.clone();
        let label = body.label.clone();
        let rid = repo_id.clone();

        let result = crate::indexer::pipeline::run_indexation(
            &db,
            &search,
            &rid,
            std::path::Path::new(&repo_path),
            &commit,
            label.as_deref(),
            embedder,
            &exclusion_patterns,
        )
        .await?;

        Ok(Json(result))
    }

    #[post("/{repo_id}/fetch")]
    async fn fetch_repo(
        &self,
        Path(repo_id): Path<String>,
    ) -> Result<Json<FetchRepoResponse>, GitdocError> {
        let repo = self.db.get_repo(&repo_id).await?
            .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

        git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
            .await
            .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;

        Ok(Json(FetchRepoResponse { fetched: true, repo_id }))
    }

    #[delete("/{repo_id}")]
    async fn delete_repo(
        &self,
        Path(repo_id): Path<String>,
    ) -> Result<Json<DeleteRepoResponse>, GitdocError> {
        let repo = self.db.get_repo(&repo_id).await?;

        let existed = self.db.delete_repo(&repo_id).await?;
        if !existed {
            return Err(GitdocError::NotFound("repo not found".into()));
        }

        if let Some(repo) = repo {
            let _ = tokio::fs::remove_dir_all(&repo.path).await;
        }

        let gc = self.db.gc_orphans().await?;
        Ok(Json(DeleteRepoResponse { deleted: true, gc }))
    }
}
