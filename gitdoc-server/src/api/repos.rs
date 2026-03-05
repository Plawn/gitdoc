use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;
use crate::git_ops;

#[derive(Serialize)]
pub struct CreateRepoResponse {
    pub id: String,
}

#[derive(Serialize)]
pub struct GetRepoResponse {
    pub repo: crate::db::RepoRow,
    pub snapshots: Vec<crate::db::SnapshotRow>,
}

#[derive(Serialize)]
pub struct FetchRepoResponse {
    pub fetched: bool,
    pub repo_id: String,
}

#[derive(Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
    pub gc: crate::db::GcStats,
}

#[derive(Deserialize)]
pub struct CreateRepoBody {
    pub id: String,
    pub name: String,
    pub url: String,
}

pub async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRepoBody>,
) -> Result<(StatusCode, Json<CreateRepoResponse>), GitdocError> {
    if body.id.is_empty() || body.name.is_empty() {
        return Err(GitdocError::BadRequest("id and name must be non-empty".into()));
    }
    if body.url.is_empty() {
        return Err(GitdocError::BadRequest("url must be non-empty".into()));
    }

    // Clone into repos_dir/{repo_id}
    let dest = git_ops::repo_clone_path(&state.config.repos_dir, &body.id);
    if let Err(e) = git_ops::clone_repo(&body.url, &dest).await {
        // Clean up partial clone
        let _ = tokio::fs::remove_dir_all(&dest).await;
        return Err(GitdocError::BadRequest(format!("clone failed: {e}")));
    }
    let repo_path = dest.to_string_lossy().into_owned();

    state
        .db
        .insert_repo(&body.id, &repo_path, &body.name, Some(&body.url))
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateRepoResponse { id: body.id }),
    ))
}

pub async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::db::RepoRow>>, GitdocError> {
    let repos = state.db.list_repos().await?;
    Ok(Json(repos))
}

pub async fn get_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<GetRepoResponse>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;
    let snapshots = state.db.list_snapshots(&repo_id).await.unwrap_or_default();
    Ok(Json(GetRepoResponse { repo, snapshots }))
}

#[derive(Deserialize)]
pub struct IndexBody {
    #[serde(default = "default_commit")]
    pub commit: String,
    pub label: Option<String>,
    #[serde(default)]
    pub fetch: bool,
}

fn default_commit() -> String {
    "HEAD".to_string()
}

pub async fn index_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(body): Json<IndexBody>,
) -> Result<Json<crate::indexer::pipeline::IndexResult>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

    // Auto-fetch if requested
    if body.fetch {
        git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
            .await
            .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;
    }

    let db = Arc::clone(&state.db);
    let search = Arc::clone(&state.search);
    let embedder = state.embedder.clone();
    let exclusion_patterns = state.config.exclusion_patterns.clone();
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

pub async fn fetch_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<FetchRepoResponse>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

    git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
        .await
        .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;

    Ok(Json(FetchRepoResponse { fetched: true, repo_id }))
}

pub async fn delete_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<DeleteResponse>, GitdocError> {
    // Look up repo before deleting to check if it has a URL (cloned dir to clean up)
    let repo = state.db.get_repo(&repo_id).await?;

    let existed = state.db.delete_repo(&repo_id).await?;
    if !existed {
        return Err(GitdocError::NotFound("repo not found".into()));
    }

    // Clean up cloned directory
    if let Some(repo) = repo {
        let _ = tokio::fs::remove_dir_all(&repo.path).await;
    }

    let gc = state.db.gc_orphans().await?;
    Ok(Json(DeleteResponse { deleted: true, gc }))
}
