use std::net::SocketAddr;
use std::path::PathBuf;

use r2e::prelude::ConfigProperties;

// ─── Internal domain types (unchanged) ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum ArchitectMode {
    Auto,
    ToolsOnly,
}

pub struct EmbeddingConfig {
    pub provider: String,
    pub api_key: String,
}

pub struct LlmConfig {
    pub engine: llm_ai::EngineConfig,
}

pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub index_path: PathBuf,
    pub repos_dir: PathBuf,
    pub log_format: String,
    pub exclusion_patterns: Vec<String>,
    pub embedding: Option<EmbeddingConfig>,
    pub llm: Option<LlmConfig>,
    pub max_prompt_tokens: usize,
    pub condensation_threshold: usize,
    pub architect_mode: ArchitectMode,
}

// ─── r2e ConfigProperties ───

/// Root config loaded by `load_config::<RootConfig>()`.
/// Wraps all gitdoc-specific config under the `gitdoc` YAML key, so
/// env var auto-mapping works: `GITDOC_BIND_ADDR` ↔ `gitdoc.bind_addr`.
#[derive(ConfigProperties, Clone, Debug)]
pub struct RootConfig {
    #[config(section)]
    pub gitdoc: GitdocConfig,
}

/// Typed configuration matching the `gitdoc:` section of `application.yaml`.
/// Auto-registered as a bean by `load_config::<RootConfig>()`.
#[derive(ConfigProperties, Clone, Debug)]
pub struct GitdocConfig {
    #[config(default = "127.0.0.1:3000")]
    pub bind_addr: String,

    #[config(default = "postgres://localhost/gitdoc")]
    pub database_url: String,

    #[config(default = "./gitdoc_index")]
    pub index_path: String,

    #[config(default = "./gitdoc_repos")]
    pub repos_dir: String,

    #[config(default = "text")]
    pub log_format: String,

    #[config(default = 12000)]
    pub max_prompt_tokens: usize,

    #[config(default = 6000)]
    pub condensation_threshold: usize,

    #[config(default = "tools_only")]
    pub architect_mode: String,

    pub exclusion_patterns: Option<Vec<String>>,

    #[config(section)]
    pub embedding: Option<EmbeddingSection>,

    #[config(section)]
    pub llm: Option<LlmSection>,
}

#[derive(ConfigProperties, Clone, Debug)]
pub struct EmbeddingSection {
    pub provider: String,
    pub api_key: String,
}

#[derive(ConfigProperties, Clone, Debug)]
pub struct LlmSection {
    #[config(default = "azure")]
    pub kind: String,
    pub endpoint: String,
    pub key: Option<String>,
    pub model: Option<String>,
}

// ─── Conversion: GitdocConfig → Config ───

fn default_exclusion_patterns() -> Vec<String> {
    vec![
        "node_modules/".into(),
        "target/".into(),
        ".git/".into(),
        "vendor/".into(),
        ".next/".into(),
        "dist/".into(),
        "build/".into(),
        "__pycache__/".into(),
    ]
}

impl Config {
    /// Build the internal `Config` from a r2e-loaded `GitdocConfig`.
    /// Preserves backward compatibility: env vars `COHERE_KEY`, `OPENAI_API_KEY`,
    /// and `GITDOC_LLM_*` still work even without YAML sections.
    pub fn from_gitdoc_config(gc: &GitdocConfig) -> Self {
        let bind_addr = gc.bind_addr.parse().expect("invalid bind_addr");

        let mut exclusion_patterns = default_exclusion_patterns();
        if let Some(extra) = &gc.exclusion_patterns {
            for p in extra {
                if !exclusion_patterns.contains(p) {
                    exclusion_patterns.push(p.clone());
                }
            }
        }

        let embedding = Self::resolve_embedding(&gc.embedding);
        let llm = Self::resolve_llm(&gc.llm);
        let architect_mode = gc.architect_mode.parse().unwrap_or(ArchitectMode::ToolsOnly);

        Self {
            bind_addr,
            database_url: gc.database_url.clone(),
            index_path: PathBuf::from(&gc.index_path),
            repos_dir: PathBuf::from(&gc.repos_dir),
            log_format: gc.log_format.clone(),
            exclusion_patterns,
            embedding,
            llm,
            max_prompt_tokens: gc.max_prompt_tokens,
            condensation_threshold: gc.condensation_threshold,
            architect_mode,
        }
    }

    fn resolve_embedding(section: &Option<EmbeddingSection>) -> Option<EmbeddingConfig> {
        // Env vars take priority (backward compat)
        if let Ok(key) = std::env::var("COHERE_KEY") {
            return Some(EmbeddingConfig { provider: "cohere".into(), api_key: key });
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            return Some(EmbeddingConfig { provider: "openai".into(), api_key: key });
        }
        // Fall back to config section
        section.as_ref().map(|s| EmbeddingConfig {
            provider: s.provider.clone(),
            api_key: s.api_key.clone(),
        })
    }

    fn resolve_llm(section: &Option<LlmSection>) -> Option<LlmConfig> {
        // Env vars take priority, fall back to section values
        let endpoint = std::env::var("GITDOC_LLM_ENDPOINT").ok()
            .or_else(|| section.as_ref().map(|s| s.endpoint.clone()));
        let key = std::env::var("GITDOC_LLM_KEY").ok()
            .or_else(|| section.as_ref().and_then(|s| s.key.clone()));
        let model = std::env::var("GITDOC_LLM_MODEL").ok()
            .or_else(|| section.as_ref().and_then(|s| s.model.clone()));
        let kind_str = std::env::var("GITDOC_LLM_KIND").ok()
            .or_else(|| section.as_ref().map(|s| s.kind.clone()))
            .unwrap_or_else(|| "azure".into());

        let endpoint = endpoint?;

        let kind = match kind_str.as_str() {
            "azure_inference" => llm_ai::EngineKind::AzureInference,
            "ollama" => llm_ai::EngineKind::Ollama,
            _ => llm_ai::EngineKind::Azure,
        };

        Some(LlmConfig {
            engine: llm_ai::EngineConfig {
                name: "gitdoc-llm".into(),
                kind,
                endpoint,
                key,
                deployment: model,
                detect: None,
                supports_temperature: true,
                thinking: None,
                reasoning_effort: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_exclusion_patterns_populated() {
        let patterns = default_exclusion_patterns();
        assert!(patterns.contains(&"node_modules/".to_string()));
        assert!(patterns.contains(&"target/".to_string()));
        assert!(patterns.contains(&".git/".to_string()));
        assert_eq!(patterns.len(), 8);
    }

    #[test]
    fn from_gitdoc_config_defaults() {
        let gc = GitdocConfig {
            bind_addr: "127.0.0.1:3000".into(),
            database_url: "postgres://localhost/gitdoc".into(),
            index_path: "./gitdoc_index".into(),
            repos_dir: "./gitdoc_repos".into(),
            log_format: "text".into(),
            max_prompt_tokens: 12000,
            condensation_threshold: 6000,
            architect_mode: "tools_only".into(),
            exclusion_patterns: None,
            embedding: None,
            llm: None,
        };
        let cfg = Config::from_gitdoc_config(&gc);
        assert_eq!(cfg.bind_addr.to_string(), "127.0.0.1:3000");
        assert_eq!(cfg.exclusion_patterns.len(), 8);
        assert!(cfg.embedding.is_none());
        assert!(cfg.llm.is_none());
        assert_eq!(cfg.architect_mode, ArchitectMode::ToolsOnly);
    }

    #[test]
    fn from_gitdoc_config_with_extra_exclusions() {
        let gc = GitdocConfig {
            bind_addr: "0.0.0.0:8080".into(),
            database_url: "postgres://custom/db".into(),
            index_path: "./custom_index".into(),
            repos_dir: "./custom_repos".into(),
            log_format: "json".into(),
            max_prompt_tokens: 8000,
            condensation_threshold: 4000,
            architect_mode: "auto".into(),
            exclusion_patterns: Some(vec!["custom_dir/".into()]),
            embedding: None,
            llm: None,
        };
        let cfg = Config::from_gitdoc_config(&gc);
        assert_eq!(cfg.bind_addr.to_string(), "0.0.0.0:8080");
        assert_eq!(cfg.log_format, "json");
        assert_eq!(cfg.max_prompt_tokens, 8000);
        assert_eq!(cfg.architect_mode, ArchitectMode::Auto);
        assert!(cfg.exclusion_patterns.contains(&"custom_dir/".to_string()));
        assert_eq!(cfg.exclusion_patterns.len(), 9); // 8 defaults + 1 custom
    }

    #[test]
    fn from_gitdoc_config_with_embedding_section() {
        let gc = GitdocConfig {
            bind_addr: "127.0.0.1:3000".into(),
            database_url: "postgres://localhost/gitdoc".into(),
            index_path: "./gitdoc_index".into(),
            repos_dir: "./gitdoc_repos".into(),
            log_format: "text".into(),
            max_prompt_tokens: 12000,
            condensation_threshold: 6000,
            architect_mode: "tools_only".into(),
            exclusion_patterns: None,
            embedding: Some(EmbeddingSection {
                provider: "cohere".into(),
                api_key: "test-key".into(),
            }),
            llm: None,
        };

        // Only test section fallback when no env vars are set
        // (env vars COHERE_KEY/OPENAI_API_KEY take priority)
        if std::env::var("COHERE_KEY").is_err() && std::env::var("OPENAI_API_KEY").is_err() {
            let cfg = Config::from_gitdoc_config(&gc);
            let emb = cfg.embedding.unwrap();
            assert_eq!(emb.provider, "cohere");
            assert_eq!(emb.api_key, "test-key");
        }
    }
}
