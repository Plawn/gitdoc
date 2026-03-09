#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMode {
    Simple,
    Granular,
}

#[derive(Clone)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

pub struct Config {
    pub server_url: String,
    pub mode: McpMode,
    pub basic_auth: Option<BasicAuth>,
}

impl Config {
    pub fn from_env() -> Self {
        let mode = match std::env::var("GITDOC_MCP_MODE").as_deref() {
            Ok("granular") => McpMode::Granular,
            _ => McpMode::Simple,
        };
        let basic_auth = match (
            std::env::var("GITDOC_BASIC_AUTH_USER"),
            std::env::var("GITDOC_BASIC_AUTH_PASSWORD"),
        ) {
            (Ok(username), Ok(password)) if !username.is_empty() => {
                Some(BasicAuth { username, password })
            }
            _ => None,
        };
        Self {
            server_url: std::env::var("GITDOC_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            mode,
            basic_auth,
        }
    }
}
