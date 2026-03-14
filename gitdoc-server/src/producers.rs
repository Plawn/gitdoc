use std::sync::Arc;

use r2e::prelude::*;

use crate::config::{Config, GitdocConfig};
use crate::db::Database;
use crate::embeddings::{self, EmbeddingProvider};
use crate::search::SearchIndex;

/// Converts the r2e-loaded GitdocConfig into our processed Config.
#[producer]
pub fn create_config(gc: GitdocConfig) -> Arc<Config> {
    Arc::new(Config::from_gitdoc_config(&gc))
}

/// Connects to the database and runs migrations.
#[producer]
pub async fn create_database(config: Arc<Config>) -> Arc<Database> {
    Arc::new(
        Database::connect(&config.database_url)
            .await
            .expect("failed to connect to database"),
    )
}

/// Opens the Tantivy search index.
#[producer]
pub fn create_search_index(config: Arc<Config>) -> Arc<SearchIndex> {
    Arc::new(SearchIndex::open(&config.index_path).expect("failed to open search index"))
}

/// Creates the embedding provider (returns None if not configured).
#[producer]
pub fn create_embedder(config: Arc<Config>) -> Option<Arc<dyn EmbeddingProvider>> {
    match &config.embedding {
        Some(ecfg) => match embeddings::create_provider(ecfg) {
            Ok(provider) => {
                tracing::info!(provider = %ecfg.provider, "embedding provider initialized");
                Some(Arc::from(provider))
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to create embedding provider, continuing without embeddings");
                None
            }
        },
        None => {
            tracing::info!("no embedding provider configured (set COHERE_KEY or OPENAI_API_KEY)");
            None
        }
    }
}

/// Creates the LLM client (returns None if not configured).
#[producer]
pub fn create_llm_client(config: Arc<Config>) -> Option<Arc<llm_ai::OpenAiCompatibleClient>> {
    match &config.llm {
        Some(llm_cfg) => {
            match llm_ai::ClientProvider::from_config(&[llm_cfg.engine.clone()]) {
                Ok(provider) => {
                    let client = provider.get("gitdoc-llm");
                    if client.is_some() {
                        tracing::info!(
                            endpoint = %llm_cfg.engine.endpoint,
                            model = ?llm_cfg.engine.deployment,
                            "LLM provider initialized"
                        );
                    }
                    client
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create LLM provider, continuing without LLM");
                    None
                }
            }
        }
        None => {
            tracing::info!("no LLM provider configured (set GITDOC_LLM_ENDPOINT)");
            None
        }
    }
}
