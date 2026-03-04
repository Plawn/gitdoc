//! Retry logic for LLM completion calls.
//!
//! Provides unified retry functionality with exponential backoff,
//! rate limit detection, and semaphore-based concurrency control.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::client::{CompletionMessage, CompletionResponse, OpenAiCompatibleClient};
use crate::config::CompletionRetryConfig;
use crate::error::AiClientError;
use crate::types::ResponseFormat;

// Metric name constants
const METRIC_RATE_LIMIT_TOTAL: &str = "llm_rate_limit_total";
const METRIC_RETRY_TOTAL: &str = "llm_retry_total";
const METRIC_CONCURRENCY: &str = "llm_concurrency";
const METRIC_SEMAPHORE_QUEUED: &str = "llm_semaphore_queued";
const METRIC_SEMAPHORE_WAIT_SECONDS: &str = "llm_semaphore_wait_duration_seconds";
const METRIC_PROVIDER_REQUEST_SECONDS: &str = "llm_provider_request_duration_seconds";

/// RAII guard to decrement the concurrency gauge on drop.
struct ConcurrencyGuard;

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        metrics::gauge!(METRIC_CONCURRENCY).decrement(1.0);
        tracing::trace!("ConcurrencyGuard dropped — llm_concurrency decremented");
    }
}

/// RAII guard to decrement the semaphore queued gauge on drop.
/// Tracks how many tasks are waiting for a semaphore permit.
struct SemaphoreQueuedGuard;

impl Drop for SemaphoreQueuedGuard {
    fn drop(&mut self) {
        metrics::gauge!(METRIC_SEMAPHORE_QUEUED).decrement(1.0);
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Base delay between retries in milliseconds.
    pub base_delay_ms: u64,
    /// Whether to use exponential backoff (delay doubles each retry, capped at 16x).
    pub exponential_backoff: bool,
    /// Whether to record metrics for rate limits and retries.
    pub record_metrics: bool,
    /// Log prefix for detailed logging (e.g., "[AUTO-REFINE:RETRY]").
    /// If None, uses simplified logging.
    pub log_prefix: Option<&'static str>,
}

impl RetryConfig {
    /// Create a simple retry config with fixed delay (for prompting_service compatibility).
    pub fn fixed(max_retries: u32, delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay_ms: delay.as_millis() as u64,
            exponential_backoff: false,
            record_metrics: true,
            log_prefix: None,
        }
    }

    /// Create a retry config with exponential backoff (for agent_iteration compatibility).
    pub fn exponential(max_retries: u32, base_delay_ms: u64) -> Self {
        Self {
            max_retries,
            base_delay_ms,
            exponential_backoff: true,
            record_metrics: false,
            log_prefix: Some("[AUTO-REFINE:RETRY]"),
        }
    }

    /// Create from CompletionRetryConfig.
    pub fn from_completion_config(config: &CompletionRetryConfig) -> Self {
        Self {
            max_retries: config.max_retries,
            base_delay_ms: config.retry_delay_ms,
            exponential_backoff: false,
            record_metrics: true,
            log_prefix: None,
        }
    }

    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay_ms = if self.exponential_backoff {
            // Exponential backoff: base * 2^attempt, capped at 16x (2^4)
            self.base_delay_ms * (1 << attempt.min(4))
        } else {
            self.base_delay_ms
        };
        Duration::from_millis(delay_ms)
    }
}

/// Detect rate limit errors from AI client responses.
pub fn is_rate_limit_error(error: &AiClientError) -> bool {
    error.is_rate_limit()
}

/// Execute a completion with semaphore control and configurable retries.
///
/// This function:
/// 1. Acquires a semaphore permit ONCE before the retry loop
/// 2. Retries on failure with configurable backoff strategy
/// 3. Detects rate limit errors for appropriate logging/metrics
pub async fn do_completion_with_retries(
    client: Arc<OpenAiCompatibleClient>,
    semaphore: &Semaphore,
    messages: &[CompletionMessage<'_>],
    temperature: Option<f64>,
    response_format: ResponseFormat,
    max_tokens: Option<i32>,
    config: &RetryConfig,
) -> Result<CompletionResponse, AiClientError> {
    let call_start = std::time::Instant::now();

    // Track how many tasks are waiting for a permit
    metrics::gauge!(METRIC_SEMAPHORE_QUEUED).increment(1.0);
    let queued_guard = SemaphoreQueuedGuard;

    // Acquire permit ONCE before retry loop
    let _permit = semaphore
        .acquire()
        .await
        .map_err(|_| AiClientError::request_failed("Completion semaphore closed"))?;

    // No longer queued — explicitly drop to decrement queued gauge
    drop(queued_guard);

    // Record how long we waited for the semaphore
    metrics::histogram!(METRIC_SEMAPHORE_WAIT_SECONDS)
        .record(call_start.elapsed().as_secs_f64());

    // Track active LLM calls (only after semaphore acquired)
    metrics::gauge!(METRIC_CONCURRENCY).increment(1.0);
    let _concurrency_guard = ConcurrencyGuard;

    if let Some(prefix) = config.log_prefix {
        tracing::debug!(
            max_retries = config.max_retries,
            base_delay_ms = config.base_delay_ms,
            exponential_backoff = config.exponential_backoff,
            temperature = ?temperature,
            response_format = ?response_format,
            messages_count = messages.len(),
            "{} Starting completion call", prefix
        );
    }

    let model_name = client.name().to_owned();
    let mut last_error = None;

    for attempt in 0..config.max_retries {
        let attempt_start = std::time::Instant::now();

        match client
            .complete(messages, temperature, response_format, max_tokens)
            .await
        {
            Ok(response) => {
                metrics::histogram!(
                    METRIC_PROVIDER_REQUEST_SECONDS,
                    "model" => model_name.clone(),
                    "status" => "success",
                )
                .record(attempt_start.elapsed().as_secs_f64());

                if let Some(prefix) = config.log_prefix {
                    tracing::debug!(
                        attempt = attempt + 1,
                        duration_ms = attempt_start.elapsed().as_millis() as u64,
                        total_duration_ms = call_start.elapsed().as_millis() as u64,
                        input_tokens = response.input_tokens,
                        output_tokens = response.output_tokens,
                        "{} Completion succeeded",
                        prefix
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                let is_rate_limit = e.is_rate_limit();
                let is_last_attempt = attempt + 1 >= config.max_retries;
                let backoff = config.calculate_delay(attempt);

                let error_status = if is_rate_limit {
                    "rate_limited"
                } else {
                    "error"
                };
                metrics::histogram!(
                    METRIC_PROVIDER_REQUEST_SECONDS,
                    "model" => model_name.clone(),
                    "status" => error_status,
                )
                .record(attempt_start.elapsed().as_secs_f64());

                // Record metrics if enabled
                if config.record_metrics {
                    if is_rate_limit {
                        metrics::counter!(METRIC_RATE_LIMIT_TOTAL).increment(1);
                    } else {
                        metrics::counter!(METRIC_RETRY_TOTAL).increment(1);
                    }
                }

                // Log based on configuration
                if let Some(prefix) = config.log_prefix {
                    let error_category = if is_rate_limit { "RATE_LIMIT" } else { "ERROR" };
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries = config.max_retries,
                        error_category = error_category,
                        error = %e,
                        is_rate_limit = is_rate_limit,
                        is_last_attempt = is_last_attempt,
                        next_backoff_ms = if is_last_attempt { 0 } else { backoff.as_millis() as u64 },
                        attempt_duration_ms = attempt_start.elapsed().as_millis() as u64,
                        "{} Attempt {}/{} failed: {} - {}",
                        prefix,
                        attempt + 1,
                        config.max_retries,
                        error_category,
                        if is_last_attempt { "NO MORE RETRIES" } else { "will retry" }
                    );
                } else {
                    if is_rate_limit {
                        tracing::warn!(
                            "Rate limited, attempt {}/{}",
                            attempt + 1,
                            config.max_retries
                        );
                    } else {
                        tracing::warn!(
                            "Completion failed: {} - retry in {:?}, attempt {}/{}",
                            e,
                            backoff,
                            attempt + 1,
                            config.max_retries
                        );
                    }
                }

                // Only retry on rate limit (429) - fail immediately on all other errors
                if !is_rate_limit {
                    return Err(e);
                }

                last_error = Some(e);

                // Sleep before next retry
                if !is_last_attempt {
                    if config.log_prefix.is_some() {
                        tracing::debug!(
                            backoff_ms = backoff.as_millis() as u64,
                            "Sleeping before retry..."
                        );
                    }
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    let final_error =
        last_error.unwrap_or_else(|| AiClientError::max_retries_exceeded(config.max_retries, None));

    if let Some(prefix) = config.log_prefix {
        tracing::error!(
            max_retries = config.max_retries,
            total_duration_ms = call_start.elapsed().as_millis() as u64,
            final_error = %final_error,
            "{} All {} retries exhausted, giving up",
            prefix,
            config.max_retries
        );
    }

    Err(final_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_rate_limit_error_429() {
        let error = AiClientError::api_error(429, "Too Many Requests");
        assert!(is_rate_limit_error(&error));
    }

    #[test]
    fn test_is_rate_limit_error_rate_limit_message() {
        let error = AiClientError::api_error(500, "Rate limit exceeded");
        assert!(is_rate_limit_error(&error));
    }

    #[test]
    fn test_is_rate_limit_error_other_errors() {
        let error = AiClientError::request_failed("Connection timeout");
        assert!(!is_rate_limit_error(&error));

        let error = AiClientError::api_error(500, "Internal server error");
        assert!(!is_rate_limit_error(&error));
    }

    #[test]
    fn test_retry_config_fixed_delay() {
        let config = RetryConfig::fixed(3, Duration::from_millis(100));
        assert_eq!(config.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(config.calculate_delay(1), Duration::from_millis(100));
        assert_eq!(config.calculate_delay(5), Duration::from_millis(100));
    }

    #[test]
    fn test_retry_config_exponential_backoff() {
        let config = RetryConfig::exponential(5, 100);
        assert_eq!(config.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(config.calculate_delay(1), Duration::from_millis(200));
        assert_eq!(config.calculate_delay(2), Duration::from_millis(400));
        assert_eq!(config.calculate_delay(3), Duration::from_millis(800));
        assert_eq!(config.calculate_delay(4), Duration::from_millis(1600));
        assert_eq!(config.calculate_delay(5), Duration::from_millis(1600)); // capped
    }
}
