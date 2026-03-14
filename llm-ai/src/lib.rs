pub mod client;
pub mod client_provider;
pub mod config;
pub mod error;
pub mod retry;
pub mod types;

// Re-export main public types for convenience
pub use client::{CompletionMessage, CompletionResponse, OpenAiCompatibleClient};
pub use client_provider::ClientProvider;
pub use config::{CompletionRetryConfig, EngineConfig, EngineKind};
pub use error::{AiClientError, ErrorCategory};
pub use retry::{do_completion_with_retries, is_rate_limit_error, RetryConfig};
pub use types::{ReasoningEffort, ResponseFormat, Role, ThinkingConfig};
