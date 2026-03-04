//! Configuration types for LLM engines.

use serde::Deserialize;

use crate::types::{ReasoningEffort, ThinkingConfig};

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    Azure,
    AzureInference,
    Ollama,
}

#[derive(Clone, Deserialize)]
pub struct EngineConfig {
    pub name: String,
    pub kind: EngineKind,
    pub endpoint: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub deployment: Option<String>,
    #[serde(default)]
    pub detect: Option<bool>,
    /// Whether the model supports custom temperature values.
    /// Set to false for models like GPT-5 that only support default temperature.
    /// Defaults to true.
    #[serde(default = "default_supports_temperature")]
    pub supports_temperature: bool,
    /// Extended thinking configuration (Claude-specific).
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
    /// Reasoning effort level for OpenAI models (GPT-5, o1, etc.).
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
}

fn default_supports_temperature() -> bool {
    true
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("endpoint", &self.endpoint)
            .field("key", &self.key.as_ref().map(|_| "[REDACTED]"))
            .field("deployment", &self.deployment)
            .field("detect", &self.detect)
            .field("supports_temperature", &self.supports_temperature)
            .field("thinking", &self.thinking)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionRetryConfig {
    /// Maximum number of retry attempts for failed completions
    #[serde(default = "default_completion_max_retries")]
    pub max_retries: u32,
    /// Delay between retries in milliseconds
    #[serde(default = "default_completion_retry_delay_ms")]
    pub retry_delay_ms: u64,
}

impl Default for CompletionRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_completion_max_retries(),
            retry_delay_ms: default_completion_retry_delay_ms(),
        }
    }
}

fn default_completion_max_retries() -> u32 {
    10
}

fn default_completion_retry_delay_ms() -> u64 {
    10_000 // 10 seconds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_kind_deserialize_kebab_case() {
        let azure: EngineKind = serde_json::from_str("\"azure\"").unwrap();
        assert_eq!(azure, EngineKind::Azure);

        let azure_inf: EngineKind = serde_json::from_str("\"azure-inference\"").unwrap();
        assert_eq!(azure_inf, EngineKind::AzureInference);

        let ollama: EngineKind = serde_json::from_str("\"ollama\"").unwrap();
        assert_eq!(ollama, EngineKind::Ollama);
    }

    #[test]
    fn test_engine_config_debug_redacts_key() {
        let config = EngineConfig {
            name: "azure-prod".to_string(),
            kind: EngineKind::Azure,
            endpoint: "https://example.openai.azure.com".to_string(),
            key: Some("super-secret-api-key-12345".to_string()),
            deployment: Some("gpt-4".to_string()),
            detect: Some(true),
            supports_temperature: true,
            thinking: None,
            reasoning_effort: None,
        };

        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("super-secret-api-key-12345"));
    }

    #[test]
    fn test_completion_retry_config_default() {
        let config = CompletionRetryConfig::default();
        assert_eq!(config.max_retries, 10);
        assert_eq!(config.retry_delay_ms, 10_000);
    }
}
