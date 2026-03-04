mod repos;
mod snapshots;
mod files;
mod symbols;
mod docs;
mod refs;
mod embeddings;
mod summaries;
mod gc;
pub mod types;

pub use types::*;

use anyhow::Result;
use sqlx::postgres::PgPool;

pub struct Database {
    pub(crate) pool: PgPool,
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}
