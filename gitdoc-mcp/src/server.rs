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
struct RegisterRepoParams {
    /// A unique identifier for the repository (e.g. "my-project")
    id: String,
    /// Git clone URL (e.g. "https://github.com/user/repo.git"). Use this for remote repos — the server will clone and manage the directory. Provide either 'url' or 'path', not both.
    url: Option<String>,
    /// Absolute path to an EXISTING git repository already on disk. Use this for local repos. Provide either 'url' or 'path', not both.
    path: Option<String>,
    /// Human-readable name for the repository (e.g. "My Project")
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct FetchRepoParams {
    /// The repo ID to fetch latest changes for (must be a URL-cloned repo)
    repo_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct IndexRepoParams {
    /// The repo ID to index
    repo_id: String,
    /// Git commit/ref to index (default: HEAD)
    commit: Option<String>,
    /// Optional human-readable label for the snapshot (e.g. "v1.0", "before-refactor"). Used later to reference this snapshot via the 'ref' parameter.
    label: Option<String>,
    /// If true and the repo was registered with a URL, fetch latest changes from the remote before indexing. Has no effect on local-path repos. Default: false.
    fetch: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct RepoRefParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadDocParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// File path of the doc to read (e.g. "README.md")
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct ListSymbolsParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
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
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
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
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
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
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DiffSymbolsParams {
    /// The repo ID
    repo_id: String,
    /// Optional ref for the 'from' snapshot: label, SHA prefix, or omit for latest
    from_ref: Option<String>,
    /// Optional ref for the 'to' snapshot: label, SHA prefix, or omit for latest
    to_ref: Option<String>,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    kind: Option<String>,
    /// Include private symbols (default: false)
    include_private: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct DeleteSnapshotParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchDocsParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Search query text
    query: String,
    /// Maximum number of results (default: 10)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchSymbolsParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Search query text
    query: String,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    kind: Option<String>,
    /// Filter by visibility (e.g. "pub", "private")
    visibility: Option<String>,
    /// Maximum number of results (default: 10)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SemanticSearchParams {
    /// The repo ID
    repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    reference: Option<String>,
    /// Natural language query (e.g. "how is authentication handled?")
    query: String,
    /// Search scope: "all", "docs", or "symbols" (default: "all")
    scope: Option<String>,
    /// Maximum number of results (default: 10)
    limit: Option<usize>,
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

    #[tool(description = "Check if the GitDoc server is reachable. Returns 'pong' on success. Call this first if other tools return connection errors.")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        match self.client.health().await {
            Ok(resp) if resp.trim() == "ok" => text_result("pong".into()),
            Ok(resp) => text_result(format!("server responded: {resp}")),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all registered repositories with their IDs, names, paths, and clone URLs. Use this to discover available repos before querying them.")]
    async fn list_repos(&self) -> Result<CallToolResult, McpError> {
        match self.client.list_repos().await {
            Ok(repos) => text_result(serde_json::to_string_pretty(&repos).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Register a git repository so it can be indexed. Provide exactly one of: 'url' (for remote repos — the server clones it) or 'path' (absolute filesystem path to a repo already on disk). IMPORTANT: After registering, you MUST call index_repo to create a snapshot before any query tool will work. Example: register_repo(id: 'myapp', name: 'My App', path: '/home/user/myapp') then index_repo(repo_id: 'myapp').")]
    async fn register_repo(
        &self,
        params: Parameters<RegisterRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match self.client.create_repo(&p.id, &p.name, p.url.as_deref(), p.path.as_deref()).await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Pull the latest changes from the remote for a URL-cloned repository (runs git fetch + reset to origin/HEAD). Only works for repos registered with a 'url', not 'path'. This updates the local clone but does NOT re-index — call index_repo afterwards to create a new snapshot with the updated code. Alternatively, use index_repo with fetch=true to do both in one step.")]
    async fn fetch_repo(
        &self,
        params: Parameters<FetchRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match self.client.fetch_repo(&p.repo_id).await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Index a repository at a given commit to create a snapshot. A snapshot captures all docs and code symbols at that point in time. IMPORTANT: You MUST call this at least once after register_repo before any query tool will work. If the repo was registered with a URL, set fetch=true to pull the latest changes before indexing. Use 'label' to give the snapshot a human-readable name (e.g. 'v1.0', 'main') for easy reference later via the 'ref' parameter. Returns the snapshot_id, commit SHA, and stats (file/doc/symbol/embedding counts). If the commit was already indexed, returns the existing snapshot (deduplication).")]
    async fn index_repo(
        &self,
        params: Parameters<IndexRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let commit = p.commit.as_deref().unwrap_or("HEAD");
        match self.client.index_repo(&p.repo_id, commit, p.label.as_deref(), p.fetch.unwrap_or(false)).await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get a high-level overview of a repository snapshot. Returns: snapshot metadata, full README content, list of all documentation files, and top-level public symbols. This is the best starting point for understanding a repo before drilling into specific docs or symbols.")]
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

    #[tool(description = "List all documentation files (markdown, text, etc.) in a repository snapshot. Returns file paths and titles. Use read_doc to get the full content of a specific doc.")]
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

    #[tool(description = "Read the full content of a documentation file by its path (as returned by list_docs or get_repo_overview). Returns the file's title and full text content.")]
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

    #[tool(description = "List code symbols in a repository snapshot. Returns for each symbol: id (use with get_symbol), name, qualified_name, kind, visibility, file_path, start_line, end_line, signature, doc_comment. Filters: kind (function/struct/class/trait/interface/enum/type_alias/const/static/module/macro), file_path (exact match to list symbols in one file), include_private (default false, set true to also see non-public symbols). To read a symbol's full source body, call get_symbol(symbol_id: <id>).")]
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

    #[tool(description = "Get full detail for a specific symbol by its numeric ID (integer, obtained from list_symbols, search_symbols, or find_references results). Returns: name, qualified_name, kind, visibility, signature, doc_comment, file_path, line range, the FULL SOURCE BODY, and a list of child symbols (e.g. methods of a struct, variants of an enum). Does NOT require repo_id or ref — symbol IDs are globally unique across all snapshots.")]
    async fn get_symbol(
        &self,
        params: Parameters<GetSymbolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.get_symbol(params.0.symbol_id).await {
            Ok(detail) => text_result(serde_json::to_string_pretty(&detail).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Find all symbols that reference a given symbol — i.e. 'who calls/uses this?'. Requires both symbol_id (integer) AND repo_id (to scope the search within a snapshot). Returns inbound references: other symbols that call, import, or depend on the target. Each result includes the referencing symbol's id, name, kind, file_path, and the reference kind. Filter by ref kind: 'calls' (function calls), 'type_ref' (type usage), 'implements' (trait/interface impl), 'imports' (import statements).")]
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

    #[tool(description = "Get the dependencies of a given symbol — i.e. 'what does this call/use?'. Requires both symbol_id (integer) AND repo_id. Returns outbound references: symbols that the target calls, imports, or depends on. Each result includes the referenced symbol's id, name, kind, file_path, and the reference kind. Filter by ref kind: 'calls', 'type_ref', 'implements', 'imports'.")]
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

    #[tool(description = "Find implementations of a trait/interface (pass the trait's symbol_id to see all types that implement it), or find which trait/interface a concrete type implements (pass the type's symbol_id). Works bidirectionally.")]
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

    #[tool(description = "Compare symbols between two snapshots of the same repo to see what changed. Shows added, removed, and modified symbols (with details on what fields changed — signature, visibility). Useful for understanding what changed between two commits or releases. Both from_ref and to_ref are optional and resolve like the 'ref' parameter (label, SHA prefix, or latest).")]
    async fn diff_symbols(
        &self,
        params: Parameters<DiffSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let from_id = match resolve_snapshot(&self.client, &p.repo_id, p.from_ref.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving 'from' snapshot: {e}")),
        };
        let to_id = match resolve_snapshot(&self.client, &p.repo_id, p.to_ref.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving 'to' snapshot: {e}")),
        };
        match self.client.diff_symbols(from_id, to_id, p.kind.as_deref(), p.include_private).await {
            Ok(diff) => text_result(serde_json::to_string_pretty(&diff).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Delete a snapshot and garbage-collect its orphaned data (files, docs, symbols, refs, embeddings). Resolves the snapshot via repo_id and optional ref (label, SHA prefix, or latest). This is irreversible.")]
    async fn delete_snapshot(
        &self,
        params: Parameters<DeleteSnapshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.delete_snapshot(snapshot_id).await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Manually run garbage collection to clean up orphaned data (files, docs, symbols, refs, embeddings) that are no longer referenced by any snapshot. Usually not needed — delete_snapshot already runs GC automatically.")]
    async fn gc(&self) -> Result<CallToolResult, McpError> {
        match self.client.gc().await {
            Ok(result) => text_result(serde_json::to_string_pretty(&result).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Full-text keyword search across documentation files in a snapshot. Returns matching documents with their file path, title, and highlighted text snippets containing the matches. Best for finding specific terms or phrases in docs.")]
    async fn search_docs(
        &self,
        params: Parameters<SearchDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.search_docs(snapshot_id, &p.query, p.limit).await {
            Ok(results) => text_result(serde_json::to_string_pretty(&results).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Full-text keyword search across code symbols. Matches against symbol names, signatures, and doc comments. Returns for each match: symbol_id (integer — use with get_symbol to read full source), name, qualified_name, kind, visibility, signature, file_path, and relevance score. Filters: kind (e.g. 'function'), visibility (e.g. 'pub'). This is the fastest way to find a specific function/struct/class by name.")]
    async fn search_symbols(
        &self,
        params: Parameters<SearchSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.search_symbols(snapshot_id, &p.query, p.kind.as_deref(), p.visibility.as_deref(), p.limit).await {
            Ok(results) => text_result(serde_json::to_string_pretty(&results).unwrap()),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Semantic search by meaning across docs and code using vector embeddings. Use natural language queries like 'how is authentication handled?' or 'error retry logic' to find relevant content even without exact keyword matches. Scope: 'all' (default), 'docs' (only documentation), or 'symbols' (only code). Requires an embedding provider configured on the server (COHERE_KEY or OPENAI_API_KEY). Returns error if no provider is available — fall back to search_docs or search_symbols instead.")]
    async fn semantic_search(
        &self,
        params: Parameters<SemanticSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match resolve_snapshot(&self.client, &p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(format!("error resolving snapshot: {e}")),
        };
        match self.client.semantic_search(snapshot_id, &p.query, p.scope.as_deref(), p.limit).await {
            Ok(results) => text_result(serde_json::to_string_pretty(&results).unwrap()),
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
            instructions: Some(r#"# GitDoc MCP — Code Intelligence for LLM Agents

GitDoc indexes git repositories and exposes their documentation, code symbols, and cross-references as structured data. You NEVER read raw source files — instead, you navigate through extracted symbols, docs, and a dependency graph.

Supported languages: Rust (.rs), TypeScript (.ts/.tsx), JavaScript (.js/.jsx), Markdown (.md/.mdx).

## Quick Start — MANDATORY steps before querying

You MUST follow these steps for any new repository:

1. `list_repos` → check if the repo is already registered
2. If not registered: `register_repo` with `path` (local repo) or `url` (remote — server clones it)
3. `index_repo` → creates a snapshot. **Nothing works without at least one snapshot.**
4. Now you can query: `get_repo_overview`, `list_symbols`, `search_symbols`, etc.

Example — register and index a local repo:
  register_repo(id: "myapp", name: "My App", path: "/home/user/myapp")
  index_repo(repo_id: "myapp")
  get_repo_overview(repo_id: "myapp")

Example — register a remote repo:
  register_repo(id: "tokio", name: "Tokio", url: "https://github.com/tokio-rs/tokio.git")
  index_repo(repo_id: "tokio", fetch: true, label: "latest")
  get_repo_overview(repo_id: "tokio", ref: "latest")

## Core Concepts

- **repo_id**: A string you choose when registering (e.g. "myapp"). Used in all subsequent tool calls.
- **Snapshot**: An indexed capture of a repo at a specific commit. Created by `index_repo`. A repo can have multiple snapshots (e.g. different versions).
- **ref** (optional parameter): Selects which snapshot to query. Resolution order: (1) exact label match → (2) commit SHA prefix match → (3) omit = latest snapshot. If you only have one snapshot, you can always omit `ref`.
- **symbol_id**: A numeric ID (integer) that uniquely identifies a symbol globally. Obtained from `list_symbols`, `search_symbols`, or `find_references`. Used with `get_symbol`, `find_references`, `get_dependencies`, `get_implementations`.

## Tool Reference

### Discovery & Setup
| Tool | When to use | Key params |
|------|-------------|------------|
| `ping` | Connection check | — |
| `list_repos` | See what's registered | — |
| `register_repo` | Add a new repo | `id`, `name`, and ONE of `url` or `path` |
| `index_repo` | Create a snapshot (REQUIRED before querying) | `repo_id`, optional: `commit`, `label`, `fetch` |
| `fetch_repo` | Update a URL-cloned repo (does NOT re-index) | `repo_id` |

### Browsing (start here to explore a repo)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_repo_overview` | **Best starting point.** Understand repo structure | README content, doc file listing, top-level public symbols |
| `list_docs` | Browse documentation files | File paths and titles |
| `read_doc` | Read a specific doc file | Full text content with title |
| `list_symbols` | Browse code symbols | name, kind, visibility, signature, file_path, line numbers, doc_comment |
| `get_symbol` | Read a symbol's implementation | Full source body + child symbols (methods, fields) |

### Code Navigation (trace the dependency graph)
| Tool | When to use | Returns |
|------|-------------|---------|
| `find_references` | "Who calls/uses X?" | List of symbols that reference the target (inbound) |
| `get_dependencies` | "What does X call/use?" | List of symbols the target depends on (outbound) |
| `get_implementations` | "Who implements trait T?" or "What traits does S implement?" | Implementation relationships (bidirectional) |

### Search (find things by name or meaning)
| Tool | When to use | Returns |
|------|-------------|---------|
| `search_docs` | Find docs by keyword | Matching docs with highlighted snippets |
| `search_symbols` | Find symbols by keyword (name, signature, doc comment) | Matching symbols with relevance score |
| `semantic_search` | Find by meaning ("how is auth handled?") | Docs and/or symbols ranked by semantic similarity |

### Maintenance
| Tool | When to use |
|------|-------------|
| `diff_symbols` | Compare two snapshots — see added/removed/modified symbols |
| `delete_snapshot` | Remove a snapshot (irreversible) |
| `gc` | Manually clean up orphaned data |

## Recommended Exploration Workflow

1. `list_repos` → find available repos
2. `get_repo_overview(repo_id: "X")` → read README, see doc tree and top symbols
3. `search_symbols(repo_id: "X", query: "what you're looking for")` → find relevant symbols
4. `get_symbol(symbol_id: 123)` → read the full implementation
5. `find_references(symbol_id: 123, repo_id: "X")` → see who calls it
6. `get_dependencies(symbol_id: 123, repo_id: "X")` → see what it depends on

## Common Pitfalls

- **"No snapshot found"**: You forgot to call `index_repo` first. Every repo must be indexed before querying.
- **"error resolving snapshot"**: The `ref` value doesn't match any label or commit SHA. Use `list_repos` to see available snapshots with their labels and commits.
- **semantic_search returns 503**: No embedding provider configured on the server. Use `search_docs` or `search_symbols` instead.
- **fetch_repo does nothing for local repos**: `fetch_repo` only works for URL-cloned repos. Local-path repos read directly from disk; just call `index_repo` again to capture new changes.
- **symbol_id is a number**: Don't pass a string. It's an integer returned by list/search tools.

## Symbol Kinds

Valid values for the `kind` filter: function, struct, class, trait, interface, enum, type_alias, const, static, module, macro.

## Reference Kinds

Valid values for the `kind` filter on find_references/get_dependencies: calls, type_ref, implements, imports."#.into()),
            ..Default::default()
        }
    }
}
