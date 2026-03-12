mod config;
mod client;
mod instructions;
mod mode_filter;
mod params;
mod server;
mod snapshot_resolver;
mod types;

use std::sync::Arc;
use mcp_framework::{run, McpApp, AuthProvider, CapabilityRegistry, CapabilityFilter, session::SessionStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::from_env();
    eprintln!("[gitdoc-mcp] mode={:?}, server_url={}", cfg.mode, cfg.server_url);
    let mode_filter = Arc::new(mode_filter::ModeFilter::new(cfg.mode));

    let mf = mode_filter.clone();
    run(McpApp {
        name: "gitdoc-mcp",
        auth: AuthProvider::None,
        server_factory: move |_token_store, _session_store: SessionStore<()>| {
            let client = client::GitdocClient::new(&cfg.server_url, cfg.basic_auth.as_ref());
            server::GitdocMcpServer::new(client, mf.clone())
        },
        stdio_token_env: None,
        settings: None,
        capability_registry: Some(CapabilityRegistry::new()),
        capability_filter: Some(mode_filter as Arc<dyn CapabilityFilter>),
        session_store: None,
    })
    .await
}
