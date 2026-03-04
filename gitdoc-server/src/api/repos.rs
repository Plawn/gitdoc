use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Deserialize)]
pub struct CreateRepoBody {
    pub id: String,
    pub path: String,
    pub name: String,
}

pub async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRepoBody>,
) -> impl IntoResponse {
    match state.db.insert_repo(&body.id, &body.path, &body.name) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({ "id": body.id }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn list_repos(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.list_repos() {
        Ok(repos) => (StatusCode::OK, Json(serde_json::json!(repos))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_repo(&repo_id) {
        Ok(Some(repo)) => {
            let snapshots = state.db.list_snapshots(&repo_id).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "repo": repo,
                    "snapshots": snapshots,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "repo not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
pub struct IndexBody {
    #[serde(default = "default_commit")]
    pub commit: String,
    pub label: Option<String>,
}

fn default_commit() -> String {
    "HEAD".to_string()
}

pub async fn index_repo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(body): Json<IndexBody>,
) -> impl IntoResponse {
    // Look up the repo
    let repo = match state.db.get_repo(&repo_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repo not found" })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let db = Arc::clone(&state.db);
    let repo_path = repo.path.clone();
    let commit = body.commit.clone();
    let label = body.label.clone();
    let rid = repo_id.clone();

    // Run indexation in a blocking thread
    let result = tokio::task::spawn_blocking(move || {
        crate::indexer::pipeline::run_indexation(
            &db,
            &rid,
            std::path::Path::new(&repo_path),
            &commit,
            label.as_deref(),
        )
    })
    .await;

    match result {
        Ok(Ok(index_result)) => (StatusCode::OK, Json(serde_json::json!(index_result))),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}
