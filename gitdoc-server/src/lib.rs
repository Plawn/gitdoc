pub mod config;
pub mod db;
pub mod error;
pub mod git_ops;
pub mod api;
pub mod indexer;
pub mod search;
pub mod embeddings;

use std::sync::Arc;

pub struct AppState {
    pub db: Arc<db::Database>,
    pub search: Arc<search::SearchIndex>,
    pub embedder: Option<Arc<dyn embeddings::EmbeddingProvider>>,
    pub config: Arc<config::Config>,
}
