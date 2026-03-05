use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Deserialize, Default)]
struct FileConfig {
    bind_addr: Option<String>,
    database_url: Option<String>,
    index_path: Option<String>,
    log_format: Option<String>,
    exclusion_patterns: Option<Vec<String>>,
    repos_dir: Option<String>,
    embedding: Option<FileEmbeddingConfig>,
    llm: Option<FileLlmConfig>,
    max_prompt_tokens: Option<usize>,
    condensation_threshold: Option<usize>,
    architect_mode: Option<String>,
}

#[derive(Deserialize)]
struct FileEmbeddingConfig {
    provider: Option<String>,
    api_key: Option<String>,
}

#[derive(Deserialize)]
struct FileLlmConfig {
    /// Engine kind: "azure", "azure_inference", "ollama"
    kind: Option<String>,
    /// API endpoint URL
    endpoint: Option<String>,
    /// API key
    key: Option<String>,
    /// Model/deployment name
    model: Option<String>,
}

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
    pub fn load() -> Self {
        let file_config = Self::load_file();

        let bind_addr = std::env::var("GITDOC_BIND_ADDR")
            .ok()
            .or(file_config.bind_addr)
            .unwrap_or_else(|| "127.0.0.1:3000".into())
            .parse()
            .expect("invalid bind_addr");

        let database_url = std::env::var("GITDOC_DATABASE_URL")
            .ok()
            .or(file_config.database_url)
            .unwrap_or_else(|| "postgres://localhost/gitdoc".into());

        let index_path = std::env::var("GITDOC_INDEX_PATH")
            .ok()
            .or(file_config.index_path)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./gitdoc_index"));

        let repos_dir = std::env::var("GITDOC_REPOS_DIR")
            .ok()
            .or(file_config.repos_dir)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./gitdoc_repos"));

        let log_format = std::env::var("GITDOC_LOG_FORMAT")
            .ok()
            .or(file_config.log_format)
            .unwrap_or_else(|| "text".into());

        let exclusion_patterns = {
            let mut patterns = default_exclusion_patterns();
            if let Some(extra) = file_config.exclusion_patterns {
                for p in extra {
                    if !patterns.contains(&p) {
                        patterns.push(p);
                    }
                }
            }
            patterns
        };

        let embedding = Self::resolve_embedding(&file_config.embedding);
        let llm = Self::resolve_llm(&file_config.llm);

        let max_prompt_tokens = std::env::var("GITDOC_MAX_PROMPT_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(file_config.max_prompt_tokens)
            .unwrap_or(12000);

        let condensation_threshold = std::env::var("GITDOC_CONDENSATION_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(file_config.condensation_threshold)
            .unwrap_or(6000);

        let architect_mode = match std::env::var("GITDOC_ARCHITECT_MODE")
            .ok()
            .or(file_config.architect_mode)
            .as_deref()
        {
            Some("auto") => ArchitectMode::Auto,
            _ => ArchitectMode::ToolsOnly,
        };

        Self {
            bind_addr,
            database_url,
            index_path,
            repos_dir,
            log_format,
            exclusion_patterns,
            embedding,
            llm,
            max_prompt_tokens,
            condensation_threshold,
            architect_mode,
        }
    }

    fn load_file() -> FileConfig {
        let path = std::env::var("GITDOC_CONFIG")
            .unwrap_or_else(|_| "gitdoc.toml".into());
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                tracing::warn!(path, error = %e, "failed to parse config file, using defaults");
                FileConfig::default()
            }),
            Err(_) => FileConfig::default(),
        }
    }

    fn resolve_llm(file_llm: &Option<FileLlmConfig>) -> Option<LlmConfig> {
        // Env vars: GITDOC_LLM_ENDPOINT, GITDOC_LLM_KEY, GITDOC_LLM_MODEL, GITDOC_LLM_KIND
        let endpoint = std::env::var("GITDOC_LLM_ENDPOINT").ok()
            .or_else(|| file_llm.as_ref().and_then(|f| f.endpoint.clone()));
        let key = std::env::var("GITDOC_LLM_KEY").ok()
            .or_else(|| file_llm.as_ref().and_then(|f| f.key.clone()));
        let model = std::env::var("GITDOC_LLM_MODEL").ok()
            .or_else(|| file_llm.as_ref().and_then(|f| f.model.clone()));
        let kind_str = std::env::var("GITDOC_LLM_KIND").ok()
            .or_else(|| file_llm.as_ref().and_then(|f| f.kind.clone()))
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

    fn resolve_embedding(file_embedding: &Option<FileEmbeddingConfig>) -> Option<EmbeddingConfig> {
        // env vars take priority
        if let Ok(key) = std::env::var("COHERE_KEY") {
            return Some(EmbeddingConfig { provider: "cohere".into(), api_key: key });
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            return Some(EmbeddingConfig { provider: "openai".into(), api_key: key });
        }
        // fall back to TOML
        if let Some(fe) = file_embedding {
            if let (Some(provider), Some(api_key)) = (&fe.provider, &fe.api_key) {
                return Some(EmbeddingConfig {
                    provider: provider.clone(),
                    api_key: api_key.clone(),
                });
            }
        }
        None
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
    fn file_config_from_toml() {
        let toml_str = r#"
bind_addr = "0.0.0.0:8080"
database_url = "postgres://custom/db"
log_format = "json"
exclusion_patterns = ["node_modules/", ".git/"]

[embedding]
provider = "cohere"
api_key = "test-key"
"#;
        let fc: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(fc.bind_addr.as_deref(), Some("0.0.0.0:8080"));
        assert_eq!(fc.database_url.as_deref(), Some("postgres://custom/db"));
        assert_eq!(fc.log_format.as_deref(), Some("json"));
        assert_eq!(fc.exclusion_patterns.as_ref().unwrap().len(), 2);
        assert_eq!(fc.embedding.as_ref().unwrap().provider.as_deref(), Some("cohere"));
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let fc: FileConfig = toml::from_str("").unwrap();
        assert!(fc.bind_addr.is_none());
        assert!(fc.database_url.is_none());
        assert!(fc.exclusion_patterns.is_none());
    }
}
