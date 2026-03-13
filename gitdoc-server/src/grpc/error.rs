use tonic::Status;
use crate::error::GitdocError;

impl From<GitdocError> for Status {
    fn from(err: GitdocError) -> Self {
        match err {
            GitdocError::NotFound(msg) => Status::not_found(msg),
            GitdocError::BadRequest(msg) => Status::invalid_argument(msg),
            GitdocError::Conflict(msg) => Status::already_exists(msg),
            GitdocError::ServiceUnavailable(msg) => Status::unavailable(msg),
            GitdocError::Internal(err) => Status::internal(err.to_string()),
        }
    }
}

/// Helper to convert anyhow::Error to tonic::Status.
pub fn internal(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}
