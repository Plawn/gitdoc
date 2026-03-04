//! Registry of AI clients by name.

use std::collections::HashMap;
use std::sync::Arc;

use crate::client::OpenAiCompatibleClient;
use crate::config::{EngineConfig, EngineKind};

/// Error type for client provider configuration.
#[derive(Debug, thiserror::Error)]
pub enum ClientProviderError {
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Registry of all AI clients by name
pub struct ClientProvider {
    clients: HashMap<String, Arc<OpenAiCompatibleClient>>,
    /// Pre-computed set of model names for O(1) validation lookups
    model_names: std::collections::HashSet<String>,
    /// Shared HTTP client for connection pooling (exposed for reuse by other services)
    http_client: reqwest::Client,
}

impl ClientProvider {
    pub fn from_config(engines: &[EngineConfig]) -> Result<Self, ClientProviderError> {
        let mut clients: HashMap<String, Arc<OpenAiCompatibleClient>> = HashMap::new();

        // Create a shared HTTP client for connection pooling across all AI providers.
        // pool_idle_timeout: close idle TLS connections after 90s to reclaim memory (~50-100KB each)
        // pool_max_idle_per_host: cap idle connections per host to avoid holding too many TLS buffers
        let shared_http_client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(32)
            .build()
            .expect("Failed to build HTTP client");
        tracing::info!(
            "Created shared HTTP client for {} AI engines",
            engines.len()
        );

        for engine in engines {
            let (endpoint, model) = match engine.kind {
                EngineKind::Azure => {
                    let deployment = engine.deployment.as_ref().ok_or_else(|| {
                        ClientProviderError::Config(format!(
                            "Missing 'deployment' for engine '{}'",
                            engine.name
                        ))
                    })?;
                    let endpoint = format!(
                        "{}/openai/deployments/{}/chat/completions?api-version=2024-10-01-preview",
                        engine.endpoint.trim_end_matches('/'),
                        deployment
                    );
                    (endpoint, deployment.clone())
                }
                EngineKind::AzureInference => {
                    let deployment = engine.deployment.as_ref().ok_or_else(|| {
                        ClientProviderError::Config(format!(
                            "Missing 'deployment' for engine '{}'",
                            engine.name
                        ))
                    })?;
                    let endpoint =
                        format!("{}/chat/completions", engine.endpoint.trim_end_matches('/'));
                    (endpoint, deployment.clone())
                }
                EngineKind::Ollama => {
                    let deployment = engine.deployment.as_ref().ok_or_else(|| {
                        ClientProviderError::Config(format!(
                            "Missing 'deployment' for engine '{}'",
                            engine.name
                        ))
                    })?;
                    let endpoint = format!("{}/api/chat", engine.endpoint.trim_end_matches('/'));
                    (endpoint, deployment.clone())
                }
            };

            let client = OpenAiCompatibleClient::new(
                shared_http_client.clone(),
                engine.name.clone(),
                endpoint,
                engine.key.clone(),
                model,
                engine.kind,
                engine.supports_temperature,
                engine.thinking,
                engine.reasoning_effort,
            );

            clients.insert(engine.name.clone(), Arc::new(client));

            // Build extended settings description
            let mut extended_settings = Vec::new();
            if !engine.supports_temperature {
                extended_settings.push("temperature=disabled".to_string());
            }
            if let Some(ref thinking) = engine.thinking {
                if thinking.is_enabled() {
                    let budget = thinking
                        .budget_tokens
                        .map(|b| format!(" (budget={})", b))
                        .unwrap_or_default();
                    extended_settings.push(format!("thinking=enabled{}", budget));
                }
            }
            if let Some(ref reasoning) = engine.reasoning_effort {
                if reasoning.is_enabled() {
                    extended_settings.push(format!("reasoning_effort={}", reasoning.as_str()));
                }
            }

            let settings_str = if extended_settings.is_empty() {
                "default".to_string()
            } else {
                extended_settings.join(", ")
            };

            tracing::info!(
                "Registered AI client: {} ({:?}) [{}]",
                engine.name,
                engine.kind,
                settings_str
            );
        }

        // Pre-compute model names set for O(1) validation
        let model_names = clients.keys().cloned().collect();

        Ok(Self {
            clients,
            model_names,
            http_client: shared_http_client,
        })
    }

    pub fn get(&self, name: &str) -> Option<Arc<OpenAiCompatibleClient>> {
        self.clients.get(name).cloned()
    }

    /// Get a reference to the shared HTTP client (for reuse by other services like CostTracker)
    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub fn names(&self) -> Vec<String> {
        self.clients.keys().cloned().collect()
    }

    /// Get available model names as a HashSet (for efficient validation)
    pub fn available_models(&self) -> &std::collections::HashSet<String> {
        &self.model_names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_provider_all_engine_kinds() {
        let configs = vec![
            EngineConfig {
                name: "azure-gpt4".to_string(),
                kind: EngineKind::Azure,
                endpoint: "https://myresource.openai.azure.com".to_string(),
                key: Some("test-key".to_string()),
                deployment: Some("gpt-4".to_string()),
                detect: None,
                supports_temperature: true,
                thinking: None,
                reasoning_effort: None,
            },
            EngineConfig {
                name: "azure-inf".to_string(),
                kind: EngineKind::AzureInference,
                endpoint: "https://mymodel.eastus.inference.ai.azure.com".to_string(),
                key: Some("test-key".to_string()),
                deployment: Some("phi-3".to_string()),
                detect: None,
                supports_temperature: true,
                thinking: None,
                reasoning_effort: None,
            },
            EngineConfig {
                name: "local-ollama".to_string(),
                kind: EngineKind::Ollama,
                endpoint: "http://localhost:11434".to_string(),
                key: None,
                deployment: Some("llama2".to_string()),
                detect: None,
                supports_temperature: true,
                thinking: None,
                reasoning_effort: None,
            },
        ];

        let provider = ClientProvider::from_config(&configs).unwrap();

        assert!(provider.get("azure-gpt4").is_some());
        assert!(provider.get("azure-inf").is_some());
        assert!(provider.get("local-ollama").is_some());
        assert!(provider.get("nonexistent").is_none());
        assert_eq!(provider.names().len(), 3);
    }

    #[test]
    fn test_client_provider_missing_deployment_fails() {
        let configs = vec![EngineConfig {
            name: "bad-config".to_string(),
            kind: EngineKind::Azure,
            endpoint: "https://example.com".to_string(),
            key: Some("key".to_string()),
            deployment: None,
            detect: None,
            supports_temperature: true,
            thinking: None,
            reasoning_effort: None,
        }];

        assert!(ClientProvider::from_config(&configs).is_err());
    }
}
