use gitdoc_server::{AppState, config, db};
use axum::{Router, routing::{get, post}};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = config::Config::from_env();
    let database = db::Database::open(&cfg.db_path)?;
    let state = Arc::new(AppState {
        db: Arc::new(database),
    });

    use gitdoc_server::api::snapshots;

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/repos", post(gitdoc_server::api::repos::create_repo).get(gitdoc_server::api::repos::list_repos))
        .route("/repos/{repo_id}", get(gitdoc_server::api::repos::get_repo))
        .route("/repos/{repo_id}/index", post(gitdoc_server::api::repos::index_repo))
        .route("/snapshots/{snapshot_id}/overview", get(snapshots::get_overview))
        .route("/snapshots/{snapshot_id}/docs", get(snapshots::list_docs))
        .route("/snapshots/{snapshot_id}/docs/{*path}", get(snapshots::get_doc_content))
        .route("/snapshots/{snapshot_id}/symbols", get(snapshots::list_symbols))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}", get(snapshots::get_snapshot_symbol))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/references", get(snapshots::get_symbol_references))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/implementations", get(snapshots::get_symbol_implementations))
        .route("/symbols/{symbol_id}", get(snapshots::get_symbol))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cfg.bind_addr).await?;
    tracing::info!("gitdoc-server listening on {}", cfg.bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
