use gitdoc_server::{AppState, config, db, embeddings, search};
use r2e::prelude::*;
use r2e::r2e_openapi::{OpenApiConfig, OpenApiPlugin};
use tower_http::trace::TraceLayer;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::load();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());

    if cfg.log_format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    let database = db::Database::connect(&cfg.database_url).await?;
    let search_index = search::SearchIndex::open(&cfg.index_path)?;

    let embedder: Option<Arc<dyn embeddings::EmbeddingProvider>> = match &cfg.embedding {
        Some(ecfg) => {
            match embeddings::create_provider(ecfg) {
                Ok(provider) => {
                    tracing::info!(provider = %ecfg.provider, "embedding provider initialized");
                    Some(Arc::from(provider))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create embedding provider, continuing without embeddings");
                    None
                }
            }
        }
        None => {
            tracing::info!("no embedding provider configured (set COHERE_KEY or OPENAI_API_KEY)");
            None
        }
    };

    let llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>> = match &cfg.llm {
        Some(llm_cfg) => {
            match llm_ai::ClientProvider::from_config(&[llm_cfg.engine.clone()]) {
                Ok(provider) => {
                    let client = provider.get("gitdoc-llm");
                    if client.is_some() {
                        tracing::info!(
                            endpoint = %llm_cfg.engine.endpoint,
                            model = ?llm_cfg.engine.deployment,
                            "LLM provider initialized"
                        );
                    }
                    client
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create LLM provider, continuing without LLM");
                    None
                }
            }
        }
        None => {
            tracing::info!("no LLM provider configured (set GITDOC_LLM_ENDPOINT)");
            None
        }
    };

    let bind_addr = cfg.bind_addr;

    let state = AppState {
        db: Arc::new(database),
        search: Arc::new(search_index),
        embedder,
        llm_client,
        config: Arc::new(cfg),
    };

    use gitdoc_server::api::{
        repos::RepoController,
        cheatsheet::CheatsheetController,
        snapshots::SnapshotController,
        symbols::{SnapshotSymbolController, SymbolController},
        search::{SearchController, AdminController},
        converse::ConverseController,
        summaries::SummaryController,
        explain::ExplainController,
        public_api::PublicApiController,
        module_tree::ModuleTreeController,
        type_context::TypeContextController,
        architect::{
            ArchitectLibController,
            ArchitectRuleController,
            ArchitectAdviseController,
            ArchitectCompareController,
            ArchitectProjectController,
            ArchitectDecisionController,
            ArchitectPatternController,
        },
    };

    // Health check as a plain route
    let health_route = Router::new()
        .route("/health", r2e::http::routing::get(|| async { "ok" }));

    AppBuilder::new()
        .with_state(state)
        .with(OpenApiPlugin::new(
            OpenApiConfig::new("GitDoc API", "0.1.0")
                .with_description("Code intelligence server for LLM agents")
                .with_docs_ui(true),
        ))
        // Controllers
        .register_controller::<RepoController>()
        .register_controller::<CheatsheetController>()
        .register_controller::<SnapshotController>()
        .register_controller::<SnapshotSymbolController>()
        .register_controller::<SymbolController>()
        .register_controller::<SearchController>()
        .register_controller::<AdminController>()
        .register_controller::<ConverseController>()
        .register_controller::<SummaryController>()
        .register_controller::<ExplainController>()
        .register_controller::<PublicApiController>()
        .register_controller::<ModuleTreeController>()
        .register_controller::<TypeContextController>()
        .register_controller::<ArchitectLibController>()
        .register_controller::<ArchitectRuleController>()
        .register_controller::<ArchitectAdviseController>()
        .register_controller::<ArchitectCompareController>()
        .register_controller::<ArchitectProjectController>()
        .register_controller::<ArchitectDecisionController>()
        .register_controller::<ArchitectPatternController>()
        // Plain routes
        .merge_router(health_route)
        // Layers
        .with_layer(TraceLayer::new_for_http())
        // Serve
        .serve(&bind_addr.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}
