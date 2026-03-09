use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum GitdocError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    ServiceUnavailable(String),
    Internal(anyhow::Error),
}

impl IntoResponse for GitdocError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            GitdocError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            GitdocError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            GitdocError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            GitdocError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            GitdocError::Internal(err) => {
                tracing::error!(error = %err, "internal server error");
                let message = if cfg!(debug_assertions) {
                    err.to_string()
                } else {
                    "internal server error".into()
                };
                (StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<anyhow::Error> for GitdocError {
    fn from(err: anyhow::Error) -> Self {
        GitdocError::Internal(err)
    }
}

impl From<tokio::task::JoinError> for GitdocError {
    fn from(err: tokio::task::JoinError) -> Self {
        GitdocError::Internal(err.into())
    }
}
