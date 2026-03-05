#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMode {
    Simple,
    Granular,
}

pub struct Config {
    pub server_url: String,
    pub mode: McpMode,
}

impl Config {
    pub fn from_env() -> Self {
        let mode = match std::env::var("GITDOC_MCP_MODE").as_deref() {
            Ok("granular") => McpMode::Granular,
            _ => McpMode::Simple,
        };
        Self {
            server_url: std::env::var("GITDOC_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            mode,
        }
    }
}
