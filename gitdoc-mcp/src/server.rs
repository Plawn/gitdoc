use rmcp::{
    ServerHandler,
    ErrorData as McpError,
    handler::server::router::tool::ToolRouter,
    handler::server::tool::Parameters,
    model::*,
    schemars::{self, JsonSchema},
    tool, tool_router, tool_handler,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::client::GitdocClient;
use crate::snapshot_resolver::resolve_snapshot;

#[derive(Clone)]
pub struct GitdocMcpServer {
    tool_router: ToolRouter<Self>,
    client: Arc<GitdocClient>,
}

// --- Parameter structs ---

#[derive(Deserialize, JsonSchema)]
struct IndexRepoParams {
    /// The repo ID to index
    repo_id: String,
    /// Git commit/ref to index (default: HEAD)
    commit: Option<String>,
    /// Optional human-readable label for the snapshot
    label: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct RepoRefParams {
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadDocParams {
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// File path of the doc to read (e.g. "README.md")
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct ListSymbolsParams {
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Filter by file path
    file_path: Option<String>,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    kind: Option<String>,
    /// Include private symbols (default: false)
    include_private: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct GetSymbolParams {
    /// The symbol ID (globally unique)
    symbol_id: i64,
}

#[derive(Deserialize, JsonSchema)]
struct FindReferencesParams {
    /// The symbol ID to find references for
    symbol_id: i64,
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Filter by ref kind (e.g. "calls", "type_ref", "implements")
    kind: Option<String>,
    /// Maximum number of results (default: 20)
    limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct GetDependenciesParams {
    /// The symbol ID to get dependencies for
    symbol_id: i64,
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Filter by ref kind (e.g. "calls", "type_ref")
    kind: Option<String>,
    /// Maximum number of results (default: 20)
    limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct GetImplementationsParams {
    /// The symbol ID (trait, interface, or class)
    symbol_id: i64,
    /// The repo ID
    repo_id: String,
    /// Optional ref: label, SHA prefix, or omit for latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
}

fn text_result(text: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn err_result(msg: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![Content::text(msg)]))
}

#[tool_router]
impl GitdocMcpServer {
    pub fn new(client: GitdocClient) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Arc::new(client),
        }
    }

    #[tool(description = "Ping the GitDoc server to check connectivity. Returns 'pong' if the server is reachable.")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        match self.client.health().await {
            Ok(resp) if resp.trim() == "ok" => text_result("pong".into()),
            Ok(resp) => text_result(format!("server responded: {resp}")),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all registered repositories.")]
    async fn list_repos(&self) -> Result<CallToolResult, McpError> {
        match self.client.list_repos().await {
            Ok(repos) => text_result(serde_json::to_string_pretty(&repos).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Index a repository at a given commit. Creates a snapshot with extracted docs and symbols.")]
    async fn index_repo(
        &self,
        params: Parameters<IndexRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let commit = p.commit.as_deref().unwrap_or("HEAD");
        match self.client.index_repo(&p.repo_id, commit, p.label.as_deref()).await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get an overview of a repository: snapshot info, README content, doc tree, and top-level symbols.")]
    async fn get_repo_overview(
        &self,
        params: Parameters<RepoRefParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.get_overview(snapshot_id).await {
            Ok(overview) => text_result(serde_json::to_string_pretty(&overview).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all documentation files in a repository snapshot.")]
    async fn list_docs(
        &self,
        params: Parameters<RepoRefParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.list_docs(snapshot_id).await {
            Ok(docs) => text_result(serde_json::to_string_pretty(&docs).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Read the content of a documentation file by path.")]
    async fn read_doc(
        &self,
        params: Parameters<ReadDocParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.read_doc(snapshot_id, &p.path).await {
            Ok(doc) => text_result(serde_json::to_string_pretty(&doc).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List symbols (functions, structs, classes, etc.) in a repository snapshot. Supports filtering by kind, file path, and visibility.")]
    async fn list_symbols(
        &self,
        params: Parameters<ListSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self
            .client
            .list_symbols(snapshot_id, p.kind.as_deref(), None, p.file_path.as_deref(), p.include_private)
            .await
        {
            Ok(symbols) => text_result(serde_json::to_string_pretty(&symbols).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get detailed information about a specific symbol by its ID, including its source body and children.")]
    async fn get_symbol(
        &self,
        params: Parameters<GetSymbolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.get_symbol(params.0.symbol_id).await {
            Ok(detail) => text_result(serde_json::to_string_pretty(&detail).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Find symbols that reference (call, use, depend on) a given symbol. Returns inbound references — i.e. 'who calls this?'")]
    async fn find_references(
        &self,
        params: Parameters<FindReferencesParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self
            .client
            .get_references(snapshot_id, p.symbol_id, Some("inbound"), p.kind.as_deref(), p.limit)
            .await
        {
            Ok(refs) => text_result(serde_json::to_string_pretty(&refs).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the dependencies of a given symbol — i.e. 'what does this call/use?'. Returns outbound references.")]
    async fn get_dependencies(
        &self,
        params: Parameters<GetDependenciesParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self
            .client
            .get_references(snapshot_id, p.symbol_id, Some("outbound"), p.kind.as_deref(), p.limit)
            .await
        {
            Ok(refs) => text_result(serde_json::to_string_pretty(&refs).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get implementations of a trait/interface, or the trait/interface that a type implements.")]
    async fn get_implementations(
        &self,
        params: Parameters<GetImplementationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.get_implementations(snapshot_id, p.symbol_id).await {
            Ok(impls) => text_result(serde_json::to_string_pretty(&impls).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }
}

#[tool_handler]
impl ServerHandler for GitdocMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "gitdoc-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some("GitDoc MCP - navigate codebases via tree-sitter indexed symbols".into()),
            ..Default::default()
        }
    }
}
