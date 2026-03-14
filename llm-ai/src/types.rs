//! Shared types for LLM conversations.

use serde::{Deserialize, Serialize};

use crate::config::EngineKind;

// ============================================================================
// Role
// ============================================================================

/// Message role in a conversation.
/// Each variant maps to the appropriate string for the target backend.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
    strum::Display, strum::EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum Role {
    System,
    User,
    Assistant,
    /// Developer role (OpenAI's replacement for system in some contexts)
    Developer,
    /// Function call result
    Function,
    /// Tool call result
    Tool,
}

impl Role {
    /// Get the string representation for a specific backend.
    /// Returns a static string - zero allocation.
    #[inline]
    pub fn as_str_for(&self, kind: EngineKind) -> &'static str {
        match kind {
            // All OpenAI-compatible backends use the same lowercase format
            EngineKind::Azure | EngineKind::AzureInference | EngineKind::Ollama => self.as_str(),
        }
    }

    /// Get the canonical lowercase string representation.
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Developer => "developer",
            Role::Function => "function",
            Role::Tool => "tool",
        }
    }
}

// ============================================================================
// ResponseFormat
// ============================================================================

/// Expected response format for AI completions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
#[cfg_attr(feature = "utoipa", schema(example = "TEXT"))]
pub enum ResponseFormat {
    /// Free-form text response (default).
    #[default]
    Text,
    /// Structured JSON response for programmatic processing.
    Json,
}

impl ResponseFormat {
    /// Convert to database string representation
    #[inline]
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Json => "JSON",
        }
    }
}

impl From<&str> for ResponseFormat {
    fn from(s: &str) -> Self {
        match s {
            "JSON" => Self::Json,
            _ => Self::Text,
        }
    }
}

// ============================================================================
// ThinkingConfig
// ============================================================================

/// Configuration for Claude's extended thinking mode.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Whether extended thinking is enabled.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(example = true))]
    pub enabled: bool,

    /// Budget for thinking tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(example = 5000, nullable))]
    pub budget_tokens: Option<i32>,
}

impl ThinkingConfig {
    /// Create a new ThinkingConfig with thinking enabled and optional budget.
    pub fn enabled(budget_tokens: Option<i32>) -> Self {
        Self {
            enabled: true,
            budget_tokens,
        }
    }

    /// Create a disabled ThinkingConfig.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            budget_tokens: None,
        }
    }

    /// Check if thinking is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ============================================================================
// ReasoningEffort
// ============================================================================

/// Reasoning effort level for OpenAI models (GPT-5, o1, etc.).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// No reasoning - fastest responses.
    #[default]
    None,
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra high reasoning effort - most thorough but slowest.
    #[serde(rename = "xhigh")]
    XHigh,
}

impl ReasoningEffort {
    /// Get the string value for the API request.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    /// Check if reasoning is enabled (not None).
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Role tests

    #[test]
    fn test_role_as_str() {
        assert_eq!(Role::System.as_str(), "system");
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::Developer.as_str(), "developer");
        assert_eq!(Role::Function.as_str(), "function");
        assert_eq!(Role::Tool.as_str(), "tool");
    }

    #[test]
    fn test_role_as_str_for_backends() {
        for kind in [
            EngineKind::Azure,
            EngineKind::AzureInference,
            EngineKind::Ollama,
        ] {
            assert_eq!(Role::System.as_str_for(kind), "system");
            assert_eq!(Role::User.as_str_for(kind), "user");
            assert_eq!(Role::Assistant.as_str_for(kind), "assistant");
        }
    }

    #[test]
    fn test_role_parse() {
        assert_eq!("system".parse::<Role>().unwrap(), Role::System);
        assert_eq!("USER".parse::<Role>().unwrap(), Role::User);
        assert_eq!("Assistant".parse::<Role>().unwrap(), Role::Assistant);
        assert!("invalid".parse::<Role>().is_err());
    }

    #[test]
    fn test_role_serde() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"user\"").unwrap(),
            Role::User
        );
    }

    // ResponseFormat tests

    #[test]
    fn test_response_format() {
        assert_eq!(ResponseFormat::default(), ResponseFormat::Text);
        assert_eq!(ResponseFormat::from("JSON"), ResponseFormat::Json);
        assert_eq!(ResponseFormat::from("TEXT"), ResponseFormat::Text);
        assert_eq!(ResponseFormat::Text.as_db_str(), "TEXT");
        assert_eq!(ResponseFormat::Json.as_db_str(), "JSON");
    }

    // ThinkingConfig tests

    #[test]
    fn test_thinking_config() {
        let config = ThinkingConfig::enabled(Some(5000));
        assert!(config.is_enabled());
        assert_eq!(config.budget_tokens, Some(5000));

        let disabled = ThinkingConfig::disabled();
        assert!(!disabled.is_enabled());
    }

    // ReasoningEffort tests

    #[test]
    fn test_reasoning_effort() {
        assert!(!ReasoningEffort::None.is_enabled());
        assert!(ReasoningEffort::High.is_enabled());
        assert_eq!(ReasoningEffort::High.as_str(), "high");
        assert_eq!(ReasoningEffort::XHigh.as_str(), "xhigh");
    }
}
