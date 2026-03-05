use gitdoc_server::{AppState, config, db, embeddings, search};
use axum::{Router, routing::{get, post, delete}};
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

    let state = Arc::new(AppState {
        db: Arc::new(database),
        search: Arc::new(search_index),
        embedder,
        llm_client,
        config: Arc::new(cfg),
    });

    use gitdoc_server::api::{snapshots, symbols, public_api, module_tree, type_context, summaries, converse, cheatsheet, architect};
    use gitdoc_server::api::search as search_api;

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/repos", post(gitdoc_server::api::repos::create_repo).get(gitdoc_server::api::repos::list_repos))
        .route("/repos/{repo_id}", get(gitdoc_server::api::repos::get_repo).delete(gitdoc_server::api::repos::delete_repo))
        .route("/repos/{repo_id}/index", post(gitdoc_server::api::repos::index_repo))
        .route("/repos/{repo_id}/fetch", post(gitdoc_server::api::repos::fetch_repo))
        .route("/repos/{repo_id}/cheatsheet", post(cheatsheet::generate_cheatsheet_handler).get(cheatsheet::get_cheatsheet_handler))
        .route("/repos/{repo_id}/cheatsheet/stream", post(cheatsheet::stream_generate_cheatsheet_handler))
        .route("/repos/{repo_id}/cheatsheet/patches", get(cheatsheet::list_patches_handler))
        .route("/repos/{repo_id}/cheatsheet/patches/{patch_id}", get(cheatsheet::get_patch_handler))
        .route("/snapshots/{snapshot_id}/overview", get(snapshots::get_overview))
        .route("/snapshots/{snapshot_id}/docs", get(snapshots::list_docs))
        .route("/snapshots/{snapshot_id}/docs/{*path}", get(snapshots::get_doc_content))
        .route("/snapshots/{snapshot_id}/symbols", get(symbols::list_symbols))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}", get(symbols::get_snapshot_symbol))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/references", get(symbols::get_symbol_references))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/implementations", get(symbols::get_symbol_implementations))
        .route("/snapshots/{snapshot_id}/public_api", get(public_api::get_public_api))
        .route("/snapshots/{snapshot_id}/module_tree", get(module_tree::get_module_tree))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/type_context", get(type_context::get_type_context))
        .route("/snapshots/{snapshot_id}/symbols/{symbol_id}/examples", get(type_context::get_examples))
        .route("/snapshots/{snapshot_id}/summarize", post(summaries::summarize))
        .route("/snapshots/{snapshot_id}/summary", get(summaries::get_summary))
        .route("/snapshots/{snapshot_id}/explain", get(gitdoc_server::api::explain::explain))
        .route("/snapshots/{snapshot_id}/converse", post(converse::converse))
        .route("/snapshots/{snapshot_id}/conversations", get(converse::list_conversations_handler))
        .route("/snapshots/{snapshot_id}/conversations/{conversation_id}", delete(converse::delete_conversation_handler))
        .route("/snapshots/{snapshot_id}/conversations/{conversation_id}/turns", get(converse::list_turns_handler))
        .route("/snapshots/{from_id}/diff/{to_id}", get(snapshots::diff_symbols))
        .route("/snapshots/{snapshot_id}", delete(snapshots::delete_snapshot))
        .route("/snapshots/{snapshot_id}/search/docs", get(search_api::search_docs))
        .route("/snapshots/{snapshot_id}/search/symbols", get(search_api::search_symbols))
        .route("/snapshots/{snapshot_id}/search/semantic", get(search_api::search_semantic))
        .route("/symbols/{symbol_id}", get(symbols::get_symbol))
        .route("/architect/libs", get(architect::list_libs).post(architect::create_lib))
        .route("/architect/libs/{id}", get(architect::get_lib).delete(architect::delete_lib))
        .route("/architect/libs/{id}/generate", post(architect::generate_lib_profile_handler))
        .route("/architect/rules", get(architect::list_rules).post(architect::upsert_rule))
        .route("/architect/rules/{id}", delete(architect::delete_rule))
        .route("/architect/advise", post(architect::advise))
        .route("/architect/compare", post(architect::compare))
        .route("/architect/projects", get(architect::list_projects).post(architect::create_project))
        .route("/architect/projects/{id}", get(architect::get_project).delete(architect::delete_project))
        .route("/architect/decisions", get(architect::list_decisions).post(architect::create_decision))
        .route("/architect/decisions/{id}", get(architect::get_decision).put(architect::update_decision).delete(architect::delete_decision))
        .route("/architect/patterns", get(architect::list_patterns).post(architect::create_pattern))
        .route("/architect/patterns/{id}", get(architect::get_pattern).delete(architect::delete_pattern))
        .route("/admin/gc", post(search_api::gc))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("gitdoc-server listening on {}", bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
