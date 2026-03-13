pub mod config;
pub mod db;
pub mod error;
pub mod git_ops;
pub mod api;
pub mod indexer;
pub mod search;
pub mod embeddings;
pub mod llm;
pub mod cheatsheet;
pub mod architect;
pub mod util;
pub mod grpc;

use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<db::Database>,
    pub search: Arc<search::SearchIndex>,
    pub embedder: Option<Arc<dyn embeddings::EmbeddingProvider>>,
    pub llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
    pub config: Arc<config::Config>,
}

