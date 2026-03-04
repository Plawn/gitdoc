//! OpenAI-compatible API client (works with Azure OpenAI, Azure Inference, and Ollama).

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::EngineKind;
use crate::error::AiClientError;
use crate::types::{ReasoningEffort, ResponseFormat, Role, ThinkingConfig};

/// Completion message with a typed Role and borrowed content.
#[derive(Debug, Clone, Copy)]
pub struct CompletionMessage<'a> {
    pub role: Role,
    pub content: &'a str,
}

impl<'a> CompletionMessage<'a> {
    #[inline]
    pub fn new(role: Role, content: &'a str) -> Self {
        Self { role, content }
    }

    /// Create from a role string, parsing it to Role enum.
    /// Defaults to User if parsing fails (with a warning log).
    #[inline]
    pub fn from_str_role(role: &str, content: &'a str) -> Self {
        let parsed_role = match role.parse() {
            Ok(r) => r,
            Err(_) => {
                tracing::warn!(
                    role = %role,
                    "Unknown role, defaulting to 'user'"
                );
                Role::User
            }
        };
        Self {
            role: parsed_role,
            content,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cached_tokens: i32,
    /// The thinking content from extended thinking (if enabled).
    pub thinking_content: Option<String>,
    /// Number of tokens used for thinking (if extended thinking was enabled).
    pub thinking_tokens: i32,
}

/// Authentication header configuration for different providers.
#[derive(Debug, Clone)]
enum AuthHeader {
    /// Azure-style authentication using `api-key` header.
    ApiKey(String),
    /// Bearer token authentication using `Authorization: Bearer <token>` header.
    Bearer(String),
}

/// OpenAI-compatible API client (works with Azure OpenAI, Azure Inference, and Ollama)
pub struct OpenAiCompatibleClient {
    client: Client,
    name: String,
    endpoint: String,
    /// Pre-computed authentication header (avoids allocation per request).
    auth_header: Option<AuthHeader>,
    model: String,
    kind: EngineKind,
    /// Whether this model supports custom temperature values.
    supports_temperature: bool,
    /// Extended thinking configuration (Claude-specific).
    thinking: Option<ThinkingConfig>,
    /// Reasoning effort level for OpenAI models (GPT-5, o1, etc.).
    reasoning_effort: Option<ReasoningEffort>,
}

impl std::fmt::Debug for OpenAiCompatibleClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleClient")
            .field("name", &self.name)
            .field("endpoint", &self.endpoint)
            .field(
                "auth_header",
                &self.auth_header.as_ref().map(|_| "[REDACTED]"),
            )
            .field("model", &self.model)
            .finish()
    }
}

/// Chat request with borrowed strings - no allocations needed since we serialize immediately
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormatRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<i32>,
    /// Extended thinking configuration (Claude-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingRequest>,
    /// Reasoning effort level for OpenAI models (GPT-5, o1, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
}

/// Extended thinking request configuration.
#[derive(Serialize)]
struct ThinkingRequest {
    #[serde(rename = "type")]
    thinking_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_tokens: Option<i32>,
}

/// Chat request with streaming enabled
#[derive(Serialize)]
struct StreamChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormatRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ResponseFormatRequest {
    #[serde(rename = "type")]
    format_type: &'static str,
}

/// Chat message with borrowed strings - zero allocation
#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_blocks: Option<Vec<ContentBlock>>,
}

/// Content block for extended thinking responses.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    prompt_tokens: i32,
    #[serde(default)]
    completion_tokens: i32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    thinking_tokens: Option<i32>,
}

#[derive(Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: i32,
}

impl OpenAiCompatibleClient {
    /// Create a new client with a shared HTTP client (for connection pooling)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        http_client: Client,
        name: String,
        endpoint: String,
        api_key: Option<String>,
        model: String,
        kind: EngineKind,
        supports_temperature: bool,
        thinking: Option<ThinkingConfig>,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Self {
        let auth_header = api_key.map(|key| match kind {
            EngineKind::Azure | EngineKind::AzureInference => AuthHeader::ApiKey(key),
            EngineKind::Ollama => AuthHeader::Bearer(format!("Bearer {}", key)),
        });

        Self {
            client: http_client,
            name,
            endpoint,
            auth_header,
            model,
            kind,
            supports_temperature,
            thinking,
            reasoning_effort,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn complete(
        &self,
        messages: &[CompletionMessage<'_>],
        temperature: Option<f64>,
        response_format: ResponseFormat,
        max_tokens: Option<i32>,
    ) -> Result<CompletionResponse, AiClientError> {
        let chat_messages: Vec<ChatMessage<'_>> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.as_str_for(self.kind),
                content: m.content,
            })
            .collect();

        let response_format_req = match response_format {
            ResponseFormat::Json => Some(ResponseFormatRequest {
                format_type: "json_object",
            }),
            ResponseFormat::Text => None,
        };

        let thinking_enabled = self.thinking.as_ref().is_some_and(|t| t.is_enabled());
        let effective_temperature = if self.supports_temperature && !thinking_enabled {
            temperature
        } else {
            None
        };

        let thinking_req = self.thinking.as_ref().and_then(|t| {
            if t.is_enabled() {
                Some(ThinkingRequest {
                    thinking_type: "enabled",
                    budget_tokens: t.budget_tokens,
                })
            } else {
                None
            }
        });

        let reasoning_effort_str = self.reasoning_effort.as_ref().and_then(|r| {
            if r.is_enabled() {
                Some(r.as_str())
            } else {
                None
            }
        });

        let request = ChatRequest {
            model: &self.model,
            messages: chat_messages,
            temperature: effective_temperature,
            response_format: response_format_req,
            max_tokens,
            thinking: thinking_req,
            reasoning_effort: reasoning_effort_str,
        };

        let mut req = self.client.post(&self.endpoint).json(&request);

        if let Some(auth) = &self.auth_header {
            req = match auth {
                AuthHeader::ApiKey(key) => req.header("api-key", key),
                AuthHeader::Bearer(token) => req.header("Authorization", token),
            };
        }

        let response = req
            .send()
            .await
            .map_err(|e| AiClientError::request_failed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AiClientError::api_error(status.as_u16(), body));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| AiClientError::response_read_failed(e.to_string()))?;

        tracing::debug!("API response received ({} bytes)", body.len());

        let chat_response: ChatResponse = serde_json::from_slice(&body)
            .map_err(|e| AiClientError::response_parse_failed(e.to_string()))?;

        let (content, thinking_content) = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| Self::extract_content_and_thinking(c.message))
            .unwrap_or_default();

        let usage = chat_response.usage.unwrap_or_default();
        let cached_tokens = usage
            .prompt_tokens_details
            .map(|d| d.cached_tokens)
            .unwrap_or(0);
        let thinking_tokens = usage.thinking_tokens.unwrap_or(0);

        Ok(CompletionResponse {
            content,
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            cached_tokens,
            thinking_content,
            thinking_tokens,
        })
    }

    /// Execute a streaming chat completion, returning the raw SSE response.
    pub async fn complete_stream(
        &self,
        messages: &[CompletionMessage<'_>],
        temperature: Option<f64>,
        response_format: ResponseFormat,
        max_tokens: Option<i32>,
    ) -> Result<reqwest::Response, AiClientError> {
        let chat_messages: Vec<ChatMessage<'_>> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.as_str_for(self.kind),
                content: m.content,
            })
            .collect();

        let response_format_req = match response_format {
            ResponseFormat::Json => Some(ResponseFormatRequest {
                format_type: "json_object",
            }),
            ResponseFormat::Text => None,
        };

        let thinking_enabled = self.thinking.as_ref().is_some_and(|t| t.is_enabled());
        let effective_temperature = if self.supports_temperature && !thinking_enabled {
            temperature
        } else {
            None
        };

        let thinking_req = self.thinking.as_ref().and_then(|t| {
            if t.is_enabled() {
                Some(ThinkingRequest {
                    thinking_type: "enabled",
                    budget_tokens: t.budget_tokens,
                })
            } else {
                None
            }
        });

        let reasoning_effort_str = self.reasoning_effort.as_ref().and_then(|r| {
            if r.is_enabled() {
                Some(r.as_str())
            } else {
                None
            }
        });

        let request = StreamChatRequest {
            model: &self.model,
            messages: chat_messages,
            temperature: effective_temperature,
            response_format: response_format_req,
            max_tokens,
            thinking: thinking_req,
            reasoning_effort: reasoning_effort_str,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        };

        let mut req = self.client.post(&self.endpoint).json(&request);

        if let Some(auth) = &self.auth_header {
            req = match auth {
                AuthHeader::ApiKey(key) => req.header("api-key", key),
                AuthHeader::Bearer(token) => req.header("Authorization", token),
            };
        }

        let response = req
            .send()
            .await
            .map_err(|e| AiClientError::request_failed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AiClientError::api_error(status.as_u16(), body));
        }

        Ok(response)
    }

    /// Extract content and thinking from a chat message response.
    fn extract_content_and_thinking(message: ChatMessageResponse) -> (String, Option<String>) {
        if let Some(blocks) = message.content_blocks {
            let mut text_content = String::new();
            let mut thinking = None;

            for block in blocks {
                match block {
                    ContentBlock::Thinking { thinking: t } => {
                        thinking = Some(t);
                    }
                    ContentBlock::Text { text } => {
                        if !text_content.is_empty() {
                            text_content.push('\n');
                        }
                        text_content.push_str(&text);
                    }
                }
            }

            (text_content, thinking)
        } else {
            (message.content.unwrap_or_default(), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_message_from_str_role() {
        let msg = CompletionMessage::from_str_role("user", "Hello");
        assert_eq!(msg.role, Role::User);

        let msg = CompletionMessage::from_str_role("USER", "Hello");
        assert_eq!(msg.role, Role::User);

        let msg = CompletionMessage::from_str_role("System", "Hello");
        assert_eq!(msg.role, Role::System);
    }

    #[test]
    fn test_completion_message_from_str_role_invalid_defaults_to_user() {
        let msg = CompletionMessage::from_str_role("invalid", "Hello");
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            model: "gpt-4",
            messages: vec![ChatMessage {
                role: "user",
                content: "Hello",
            }],
            temperature: Some(0.7),
            response_format: None,
            max_tokens: Some(100),
            thinking: None,
            reasoning_effort: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"temperature\":0.7"));
        assert!(!json.contains("thinking"));
        assert!(!json.contains("reasoning_effort"));
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "Hello, world!"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            Some("Hello, world!".to_string())
        );
    }

    #[test]
    fn test_extract_content_and_thinking_standard() {
        let message = ChatMessageResponse {
            content: Some("Hello, world!".to_string()),
            content_blocks: None,
        };
        let (content, thinking) = OpenAiCompatibleClient::extract_content_and_thinking(message);
        assert_eq!(content, "Hello, world!");
        assert!(thinking.is_none());
    }

    #[test]
    fn test_extract_content_and_thinking_with_blocks() {
        let message = ChatMessageResponse {
            content: None,
            content_blocks: Some(vec![
                ContentBlock::Thinking {
                    thinking: "Let me think...".to_string(),
                },
                ContentBlock::Text {
                    text: "Here is my answer.".to_string(),
                },
            ]),
        };
        let (content, thinking) = OpenAiCompatibleClient::extract_content_and_thinking(message);
        assert_eq!(content, "Here is my answer.");
        assert_eq!(thinking, Some("Let me think...".to_string()));
    }
}
