/// Category of AI client errors for classification.
///
/// Used to reduce duplication in error classification logic and provide
/// a single source of truth for error categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Rate limit exceeded (HTTP 429, throttling messages)
    RateLimit,
    /// Request or gateway timeout
    Timeout,
    /// Authentication/authorization failure (HTTP 401, 403)
    Auth,
    /// Response parsing failure
    Parse,
    /// Generic service error (catch-all for other errors)
    ServiceError,
}

/// Structured error type for AI client operations.
///
/// Replaces string-based error classification with typed variants for
/// deterministic error handling and retry logic.
#[derive(Debug, Clone)]
pub enum AiClientError {
    /// HTTP request failed to send (connection error, DNS failure, etc.)
    RequestFailed { message: String },
    /// API returned a non-success HTTP status code
    ApiError { status_code: u16, body: String },
    /// Failed to read response body
    ResponseReadFailed { message: String },
    /// Failed to parse response JSON
    ResponseParseFailed { message: String },
    /// Maximum retry attempts exceeded
    MaxRetriesExceeded {
        attempts: u32,
        last_error: Option<Box<AiClientError>>,
    },
}

impl AiClientError {
    /// Create a RequestFailed error
    pub fn request_failed(message: impl Into<String>) -> Self {
        Self::RequestFailed {
            message: message.into(),
        }
    }

    /// Create an ApiError from HTTP status and response body
    pub fn api_error(status_code: u16, body: impl Into<String>) -> Self {
        Self::ApiError {
            status_code,
            body: body.into(),
        }
    }

    /// Create a ResponseReadFailed error
    pub fn response_read_failed(message: impl Into<String>) -> Self {
        Self::ResponseReadFailed {
            message: message.into(),
        }
    }

    /// Create a ResponseParseFailed error
    pub fn response_parse_failed(message: impl Into<String>) -> Self {
        Self::ResponseParseFailed {
            message: message.into(),
        }
    }

    /// Create a MaxRetriesExceeded error
    pub fn max_retries_exceeded(attempts: u32, last_error: Option<AiClientError>) -> Self {
        Self::MaxRetriesExceeded {
            attempts,
            last_error: last_error.map(Box::new),
        }
    }

    /// Classify this error into an ErrorCategory.
    ///
    /// This is the single source of truth for error classification, used by
    /// `is_rate_limit()`, `is_timeout()`, and `is_auth_error()` methods.
    pub fn classify(&self) -> ErrorCategory {
        match self {
            Self::ApiError { status_code, body } => {
                // Check rate limit first (HTTP 429 or rate-limit messages)
                if *status_code == 429 {
                    return ErrorCategory::RateLimit;
                }
                let body_lower = body.to_lowercase();
                if body_lower.contains("rate")
                    || body_lower.contains("throttl")
                    || body_lower.contains("too many requests")
                {
                    return ErrorCategory::RateLimit;
                }

                // Check timeout (HTTP 408 or 504)
                if *status_code == 408 || *status_code == 504 {
                    return ErrorCategory::Timeout;
                }

                // Check auth errors (HTTP 401, 403 or auth messages)
                if *status_code == 401
                    || *status_code == 403
                    || body_lower.contains("unauthorized")
                    || body_lower.contains("forbidden")
                {
                    return ErrorCategory::Auth;
                }

                ErrorCategory::ServiceError
            }
            Self::RequestFailed { message } | Self::ResponseReadFailed { message } => {
                let msg_lower = message.to_lowercase();
                if msg_lower.contains("timeout") || msg_lower.contains("timed out") {
                    ErrorCategory::Timeout
                } else {
                    ErrorCategory::ServiceError
                }
            }
            Self::ResponseParseFailed { .. } => ErrorCategory::Parse,
            Self::MaxRetriesExceeded {
                last_error: Some(e),
                ..
            } => e.classify(),
            Self::MaxRetriesExceeded {
                last_error: None, ..
            } => ErrorCategory::ServiceError,
        }
    }

    /// Check if this is a rate limit error (HTTP 429 or rate-limit message)
    pub fn is_rate_limit(&self) -> bool {
        self.classify() == ErrorCategory::RateLimit
    }

    /// Check if this is a timeout error
    pub fn is_timeout(&self) -> bool {
        self.classify() == ErrorCategory::Timeout
    }

    /// Check if this is an authentication error (HTTP 401 or 403)
    pub fn is_auth_error(&self) -> bool {
        self.classify() == ErrorCategory::Auth
    }

    /// Get the HTTP status code if available
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::ApiError { status_code, .. } => Some(*status_code),
            Self::MaxRetriesExceeded {
                last_error: Some(e),
                ..
            } => e.status_code(),
            _ => None,
        }
    }
}

impl std::fmt::Display for AiClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestFailed { message } => write!(f, "Request failed: {}", message),
            Self::ApiError { status_code, body } => {
                write!(f, "API error {}: {}", status_code, body)
            }
            Self::ResponseReadFailed { message } => {
                write!(f, "Failed to read response: {}", message)
            }
            Self::ResponseParseFailed { message } => {
                write!(f, "Failed to parse response: {}", message)
            }
            Self::MaxRetriesExceeded {
                attempts,
                last_error,
            } => {
                if let Some(e) = last_error {
                    write!(
                        f,
                        "Max retries exceeded ({} attempts), last error: {}",
                        attempts, e
                    )
                } else {
                    write!(f, "Max retries exceeded ({} attempts)", attempts)
                }
            }
        }
    }
}

impl std::error::Error for AiClientError {}
