use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::error::GitdocError;
use crate::git_ops;

#[derive(Deserialize)]
pub struct CreateRepoBody {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
}

pub async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRepoBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), GitdocError> {
    if body.id.is_empty() || body.name.is_empty() {
        return Err(GitdocError::BadRequest("id and name must be non-empty".into()));
    }

    let (repo_path, url) = match (&body.url, &body.path) {
        (Some(url), None) => {
            // URL mode: clone into repos_dir/{repo_id}
            let dest = git_ops::repo_clone_path(&state.config.repos_dir, &body.id);
            if let Err(e) = git_ops::clone_repo(url, &dest).await {
                // Clean up partial clone
                let _ = tokio::fs::remove_dir_all(&dest).await;
                return Err(GitdocError::BadRequest(format!("clone failed: {e}")));
            }
            let path_str = dest.to_string_lossy().into_owned();
            (path_str, Some(url.clone()))
        }
        (None, Some(path)) => {
            if path.is_empty() {
                return Err(GitdocError::BadRequest("path must be non-empty".into()));
            }
            (path.clone(), None)
        }
        _ => {
            return Err(GitdocError::BadRequest(
                "exactly one of 'url' or 'path' must be provided".into(),
            ));
        }
    };

    state
        .db
        .insert_repo(&body.id, &repo_path, &body.name, url.as_deref())
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": body.id })),
    ))
}

pub async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let repos = state.db.list_repos().await?;
    Ok(Json(serde_json::json!(repos)))
}

pub async fn get_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;
    let snapshots = state.db.list_snapshots(&repo_id).await.unwrap_or_default();
    Ok(Json(serde_json::json!({
        "repo": repo,
        "snapshots": snapshots,
    })))
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
) -> Result<Json<serde_json::Value>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

    // Auto-fetch if requested and repo has a URL
    if body.fetch {
        if repo.url.is_some() {
            git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
                .await
                .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;
        }
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

    Ok(Json(serde_json::json!(result)))
}

pub async fn fetch_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    let repo = state.db.get_repo(&repo_id).await?
        .ok_or_else(|| GitdocError::NotFound("repo not found".into()))?;

    if repo.url.is_none() {
        return Err(GitdocError::BadRequest(
            "fetch is only supported for URL-cloned repos".into(),
        ));
    }

    git_ops::fetch_and_reset(std::path::Path::new(&repo.path))
        .await
        .map_err(|e| GitdocError::BadRequest(format!("fetch failed: {e}")))?;

    Ok(Json(serde_json::json!({ "fetched": true, "repo_id": repo_id })))
}

pub async fn delete_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<serde_json::Value>, GitdocError> {
    // Look up repo before deleting to check if it has a URL (cloned dir to clean up)
    let repo = state.db.get_repo(&repo_id).await?;

    let existed = state.db.delete_repo(&repo_id).await?;
    if !existed {
        return Err(GitdocError::NotFound("repo not found".into()));
    }

    // Clean up cloned directory if this was a URL-cloned repo
    if let Some(repo) = repo {
        if repo.url.is_some() {
            let _ = tokio::fs::remove_dir_all(&repo.path).await;
        }
    }

    let gc_stats = state.db.gc_orphans().await?;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "gc": gc_stats,
    })))
}
