mod config;
mod client;
mod params;
mod server;
mod snapshot_resolver;
mod types;

use mcp_framework::{run, McpApp, AuthProvider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::from_env();

    run(McpApp {
        name: "gitdoc-mcp",
        auth: AuthProvider::None,
        server_factory: move |_token_store| {
            let client = client::GitdocClient::new(&cfg.server_url);
            server::GitdocMcpServer::new(client)
        },
        stdio_token_env: None,
        settings: None,
        capability_registry: None,
        capability_filter: None,
    })
    .await
}
