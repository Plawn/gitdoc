pub struct Config {
    pub server_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            server_url: std::env::var("GITDOC_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
        }
    }
}
