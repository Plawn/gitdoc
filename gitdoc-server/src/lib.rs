pub mod config;
pub mod db;
pub mod api;
pub mod indexer;

use std::sync::Arc;

pub struct AppState {
    pub db: Arc<db::Database>,
}
