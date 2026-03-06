mod config;
mod client;
mod instructions;
mod mode_filter;
mod params;
mod server;
mod snapshot_resolver;
mod types;

use std::sync::Arc;
use mcp_framework::{run, McpApp, AuthProvider, CapabilityRegistry, session::SessionStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::from_env();
    let mode = cfg.mode;

    run(McpApp {
        name: "gitdoc-mcp",
        auth: AuthProvider::None,
        server_factory: move |_token_store, _session_store: SessionStore<()>| {
            let client = client::GitdocClient::new(&cfg.server_url);
            server::GitdocMcpServer::new(client, mode)
        },
        stdio_token_env: None,
        settings: None,
        capability_registry: Some(CapabilityRegistry::new()),
        capability_filter: Some(Arc::new(mode_filter::ModeFilter { mode })),
        session_store: None,
    })
    .await
}
