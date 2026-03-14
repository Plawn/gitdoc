use gitdoc_server::{AppState, config::RootConfig, llm_executor};
use r2e::prelude::*;
use r2e::r2e_grpc::{GrpcServer, AppBuilderGrpcExt};
use r2e::r2e_openapi::{OpenApiConfig, OpenApiPlugin};
use r2e::r2e_prometheus::Prometheus;
use tower_http::trace::TraceLayer;

use gitdoc_server::producers::{
    CreateConfig, CreateDatabase, CreateSearchIndex, CreateEmbedder, CreateLlmClient,
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

use gitdoc_server::grpc::{
    repos::RepoGrpcService,
    snapshots::SnapshotGrpcService,
    symbols::SymbolGrpcService,
    search::SearchGrpcService,
    analysis::AnalysisGrpcService,
    converse::ConverseGrpcService,
    cheatsheet::CheatsheetGrpcService,
    architect::ArchitectGrpcService,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Tracing must init before config loading — read log_format from env directly.
    let log_format = std::env::var("GITDOC_LOG_FORMAT").unwrap_or_else(|_| "text".into());
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());

    if log_format == "json" {
        tracing_subscriber::fmt().with_env_filter(env_filter).json().init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    // Prometheus with LLM collectors
    let mut prometheus_builder = Prometheus::builder()
        .namespace("gitdoc")
        .exclude_path("/health")
        .exclude_path("/metrics");

    for collector in llm_executor::llm_collectors() {
        prometheus_builder = prometheus_builder.register(collector);
    }

    let prometheus = prometheus_builder.build();

    // Read bind_addr from env before it's consumed by the DI graph
    let bind_addr = std::env::var("GITDOC_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into());

    let builder = AppBuilder::new()
        .plugin(prometheus)
        .plugin(GrpcServer::multiplexed())
        // Config: load application.yaml → RootConfig → GitdocConfig (auto-registered)
        .load_config::<RootConfig>()
        // Producers: construct components via DI
        .with_producer::<CreateConfig>()
        .with_producer::<CreateDatabase>()
        .with_producer::<CreateSearchIndex>()
        .with_producer::<CreateEmbedder>()
        .with_producer::<CreateLlmClient>()
        // Resolve bean graph → build AppState
        .build_state::<AppState, _, _>()
        .await;

    builder
        // Post-state plugins
        .with(Health)
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
        // gRPC services
        .register_grpc_service::<RepoGrpcService>()
        .register_grpc_service::<SnapshotGrpcService>()
        .register_grpc_service::<SymbolGrpcService>()
        .register_grpc_service::<SearchGrpcService>()
        .register_grpc_service::<AnalysisGrpcService>()
        .register_grpc_service::<ConverseGrpcService>()
        .register_grpc_service::<CheatsheetGrpcService>()
        .register_grpc_service::<ArchitectGrpcService>()
        // Layers
        .with_layer(TraceLayer::new_for_http())
        // Serve
        .serve(&bind_addr)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}
