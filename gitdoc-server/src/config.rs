use std::net::SocketAddr;
use std::path::PathBuf;

pub struct Config {
    pub bind_addr: SocketAddr,
    pub db_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let bind_addr = std::env::var("GITDOC_BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
            .parse()
            .expect("invalid GITDOC_BIND_ADDR");
        let db_path = std::env::var("GITDOC_DB_PATH")
            .unwrap_or_else(|_| "gitdoc.db".to_string())
            .into();
        Self { bind_addr, db_path }
    }
}
