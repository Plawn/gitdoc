use rmcp::{
    ServerHandler,
    ErrorData as McpError,
    handler::server::router::tool::ToolRouter,
    handler::server::tool::Parameters,
    model::*,
    service::{Peer, RequestContext, RoleServer},
    tool, tool_router, tool_handler,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use gitdoc_api_types::requests;

use crate::client::GitdocClient;
use crate::instructions::{SIMPLE_INSTRUCTIONS, GRANULAR_INSTRUCTIONS};
use crate::mode_filter::ModeFilter;
use crate::params::*;
use crate::snapshot_resolver::resolve_snapshot;

/// Per-repo conversation state: (snapshot_id, conversation_id)
type ConversationMap = Arc<Mutex<HashMap<String, (i64, i64)>>>;

#[derive(Clone)]
pub struct GitdocMcpServer {
    tool_router: ToolRouter<Self>,
    client: Arc<GitdocClient>,
    conversations: ConversationMap,
    mode_filter: Arc<ModeFilter>,
    peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
}

fn text_result(text: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn err_result(msg: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![Content::text(msg)]))
}

fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"  ));
    text_result(text)
}

#[tool_router]
impl GitdocMcpServer {
    pub fn new(client: GitdocClient, mode_filter: Arc<ModeFilter>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Arc::new(client),
            conversations: Arc::new(Mutex::new(HashMap::new())),
            mode_filter,
            peer: Arc::new(RwLock::new(None)),
        }
    }

    async fn log_info(&self, message: &str) {
        if let Some(peer) = self.peer.read().await.as_ref() {
            let _ = peer.notify_logging_message(LoggingMessageNotificationParam {
                level: LoggingLevel::Info,
                logger: Some("gitdoc".into()),
                data: serde_json::Value::String(message.to_string()),
            }).await;
        }
    }

    async fn auto_generate_cheatsheet(&self, repo_id: &str, snapshot_id: i64) {
        self.log_info("Auto-generating repo cheatsheet...").await;

        match self.client.stream_generate_cheatsheet(repo_id, snapshot_id, "auto").await {
            Ok(mut rx) => {
                while let Some(event) = rx.recv().await {
                    let msg = match event.stage.as_str() {
                        "gathering" => format!("Cheatsheet: {}", event.message),
                        "generating" => format!("Cheatsheet: {}", event.message),
                        "done" => "Cheatsheet ready".to_string(),
                        "error" => format!("Cheatsheet generation failed: {}", event.message),
                        _ => event.message.clone(),
                    };
                    self.log_info(&msg).await;
                }
            }
            Err(e) => {
                self.log_info(&format!("Cheatsheet generation skipped: {e}")).await;
            }
        }
    }

    /// Returns an error if the current mode is Simple — used to guard granular-only tools.
    fn check_granular(&self) -> Result<(), McpError> {
        if !self.mode_filter.is_granular() {
            return Err(McpError::invalid_request(
                "This tool is not available in simple mode. Use set_mode(\"granular\") or GITDOC_MCP_MODE=granular to enable all tools.",
                None,
            ));
        }
        Ok(())
    }

    /// Resolve a snapshot ID from repo_id + optional ref, returning an MCP-friendly error.
    async fn resolve_snapshot_id(&self, repo_id: &str, reference: Option<&str>) -> Result<i64, String> {
        resolve_snapshot(&self.client, repo_id, reference)
            .await
            .map_err(|e| format!("error resolving snapshot: {e}"))
    }

    #[tool(description = "Check if the GitDoc server is reachable. Returns 'pong' on success. Call this first if other tools return connection errors.")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        match self.client.health().await {
            Ok(resp) if resp.trim() == "ok" => text_result("pong".into()),
            Ok(resp) => text_result(format!("server responded: {resp}")),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Switch between 'simple' and 'granular' tool modes. Simple mode provides conversational tools (ask, architect_advise, etc.). Granular mode unlocks all tools for direct code navigation (get_symbol, find_references, search_symbols, etc.). Use 'granular' when you need exact source code or fine-grained control.")]
    async fn set_mode(
        &self,
        params: Parameters<SetModeParams>,
    ) -> Result<CallToolResult, McpError> {
        let new_granular = match params.0.mode {
            SetModeValue::Simple => false,
            SetModeValue::Granular => true,
        };

        self.mode_filter.set_granular(new_granular);

        // Notify client to re-fetch tools/list
        if let Some(peer) = self.peer.read().await.as_ref() {
            let _ = peer.notify_tool_list_changed().await;
        }

        let msg = if new_granular {
            "Switched to granular mode. The tool list has been refreshed with all tools.\n\n\
             Key tools now available:\n\
             - get_symbol, list_symbols — inspect code symbols with full source\n\
             - read_doc, list_docs — read documentation files\n\
             - find_references, get_dependencies — navigate the call graph\n\
             - search_symbols, semantic_search — targeted search\n\
             - get_module_tree, get_public_api — high-level views\n\
             - get_type_context, get_examples — deep type exploration\n\
             - explain — natural language Q&A with assembled context\n\
             - Cheatsheet, Architect KB management tools\n\n\
             Workflow: use get_module_tree or get_public_api for overview, \
             then get_symbol/find_references for details."
        } else {
            "Switched to simple mode. The tool list has been refreshed.\n\n\
             Available tools: ping, list_repos, register_repo, index_repo, \
             get_repo_overview, ask, conversation_reset, architect_advise, \
             compare_libs, get_cheatsheet, set_mode.\n\n\
             Workflow: use ask for conversational exploration, \
             get_repo_overview for structure, get_cheatsheet for accumulated knowledge."
        };
        text_result(msg.to_string())
    }

    #[tool(description = "List all registered repositories with their IDs, names, paths, and clone URLs. Use this to discover available repos before querying them.")]
    async fn list_repos(&self) -> Result<CallToolResult, McpError> {
        match self.client.list_repos().await {
            Ok(repos) => json_result(&repos),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Register a git repository by its clone URL. The server clones and manages the repository. IMPORTANT: After registering, you MUST call index_repo to create a snapshot before any query tool will work. Example: register_repo(id: 'tokio', name: 'Tokio', url: 'https://github.com/tokio-rs/tokio.git') then index_repo(repo_id: 'tokio').")]
    async fn register_repo(
        &self,
        params: Parameters<RegisterRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match self.client.create_repo(&p.id, &p.name, &p.url).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Pull the latest changes from the remote (runs git fetch + reset to origin/HEAD). This updates the server's clone but does NOT re-index — call index_repo afterwards to create a new snapshot with the updated code. Alternatively, use index_repo with fetch=true to do both in one step.")]
    async fn fetch_repo(
        &self,
        params: Parameters<FetchRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.fetch_repo(&p.repo_id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Index a repository at a given commit to create a snapshot. A snapshot captures all docs and code symbols at that point in time. IMPORTANT: You MUST call this at least once after register_repo before any query tool will work. Set fetch=true to pull the latest remote changes before indexing. Use 'label' to give the snapshot a human-readable name (e.g. 'v1.0', 'main') for easy reference later via the 'ref' parameter. Returns the snapshot_id, commit SHA, and stats (file/doc/symbol/embedding counts). If the commit was already indexed, returns the existing snapshot (deduplication).")]
    async fn index_repo(
        &self,
        params: Parameters<IndexRepoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let commit = p.commit.as_deref().unwrap_or("HEAD");
        match self.client.index_repo(&p.repo_id, commit, p.label.as_deref(), p.fetch.unwrap_or(false)).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get a high-level overview of a repository snapshot. Returns: snapshot metadata, full README content, list of all documentation files, and top-level public symbols. This is the best starting point for understanding a repo before drilling into specific docs or symbols.")]
    async fn get_repo_overview(
        &self,
        params: Parameters<RepoRefParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_overview(snapshot_id).await {
            Ok(overview) => json_result(&overview),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all documentation files (markdown, text, etc.) in a repository snapshot. Returns file paths and titles. Use read_doc to get the full content of a specific doc.")]
    async fn list_docs(
        &self,
        params: Parameters<RepoRefParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.list_docs(snapshot_id).await {
            Ok(docs) => json_result(&docs),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Read the full content of a documentation file by its path (as returned by list_docs or get_repo_overview). Returns the file's title and full text content.")]
    async fn read_doc(
        &self,
        params: Parameters<ReadDocParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.read_doc(snapshot_id, &p.path).await {
            Ok(doc) => json_result(&doc),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List code symbols in a repository snapshot. Returns for each symbol: id (use with get_symbol), name, qualified_name, kind, visibility, file_path, start_line, end_line, signature, doc_comment. Filters: kind (function/struct/class/trait/interface/enum/type_alias/const/static/module/macro), file_path (exact match to list symbols in one file), include_private (default false, set true to also see non-public symbols). To read a symbol's full source body, call get_symbol(symbol_id: <id>).")]
    async fn list_symbols(
        &self,
        params: Parameters<ListSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self
            .client
            .list_symbols(snapshot_id, p.kind.as_deref(), None, p.file_path.as_deref(), p.include_private)
            .await
        {
            Ok(symbols) => json_result(&symbols),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get full detail for a specific symbol by its numeric ID (integer, obtained from list_symbols, search_symbols, or find_references results). Returns: name, qualified_name, kind, visibility, signature, doc_comment, file_path, line range, the FULL SOURCE BODY, and a list of child symbols (e.g. methods of a struct, variants of an enum). Does NOT require repo_id or ref — symbol IDs are globally unique across all snapshots.")]
    async fn get_symbol(
        &self,
        params: Parameters<GetSymbolParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        match self.client.get_symbol(params.0.symbol_id).await {
            Ok(detail) => json_result(&detail),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Find all symbols that reference a given symbol — i.e. 'who calls/uses this?'. Requires both symbol_id (integer) AND repo_id (to scope the search within a snapshot). Returns inbound references: other symbols that call, import, or depend on the target. Each result includes the referencing symbol's id, name, kind, file_path, and the reference kind. Filter by ref kind: 'calls' (function calls), 'type_ref' (type usage), 'implements' (trait/interface impl), 'imports' (import statements).")]
    async fn find_references(
        &self,
        params: Parameters<FindReferencesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self
            .client
            .get_references(snapshot_id, p.symbol_id, Some("inbound"), p.kind.as_deref(), p.limit)
            .await
        {
            Ok(refs) => json_result(&refs),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the dependencies of a given symbol — i.e. 'what does this call/use?'. Requires both symbol_id (integer) AND repo_id. Returns outbound references: symbols that the target calls, imports, or depends on. Each result includes the referenced symbol's id, name, kind, file_path, and the reference kind. Filter by ref kind: 'calls', 'type_ref', 'implements', 'imports'.")]
    async fn get_dependencies(
        &self,
        params: Parameters<GetDependenciesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self
            .client
            .get_references(snapshot_id, p.symbol_id, Some("outbound"), p.kind.as_deref(), p.limit)
            .await
        {
            Ok(refs) => json_result(&refs),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Find implementations of a trait/interface (pass the trait's symbol_id to see all types that implement it), or find which trait/interface a concrete type implements (pass the type's symbol_id). Works bidirectionally.")]
    async fn get_implementations(
        &self,
        params: Parameters<GetImplementationsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_implementations(snapshot_id, p.symbol_id).await {
            Ok(impls) => json_result(&impls),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Compare symbols between two snapshots of the same repo to see what changed. Shows added, removed, and modified symbols (with details on what fields changed — signature, visibility). Useful for understanding what changed between two commits or releases. Both from_ref and to_ref are optional and resolve like the 'ref' parameter (label, SHA prefix, or latest).")]
    async fn diff_symbols(
        &self,
        params: Parameters<DiffSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let from_id = match self.resolve_snapshot_id(&p.repo_id, p.from_ref.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        let to_id = match self.resolve_snapshot_id(&p.repo_id, p.to_ref.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.diff_symbols(from_id, to_id, p.kind.as_deref(), p.include_private).await {
            Ok(diff) => json_result(&diff),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Full-text keyword search across documentation files in a snapshot. Returns matching documents with their file path, title, and highlighted text snippets containing the matches. Best for finding specific terms or phrases in docs.")]
    async fn search_docs(
        &self,
        params: Parameters<SearchDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.search_docs(snapshot_id, &p.query, p.limit).await {
            Ok(results) => json_result(&results),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Full-text keyword search across code symbols. Matches against symbol names, signatures, and doc comments. Returns for each match: symbol_id (integer — use with get_symbol to read full source), name, qualified_name, kind, visibility, signature, file_path, and relevance score. Filters: kind (e.g. 'function'), visibility (e.g. 'pub'). This is the fastest way to find a specific function/struct/class by name.")]
    async fn search_symbols(
        &self,
        params: Parameters<SearchSymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.search_symbols(snapshot_id, &p.query, p.kind.as_deref(), p.visibility.as_deref(), p.limit).await {
            Ok(results) => json_result(&results),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the complete public API surface of a crate or module in a SINGLE call. Returns all public symbols (functions, structs, enums, traits, consts, etc.) grouped by module, with their signatures and doc comments. Impl block methods are MERGED onto their parent types. This is the most efficient way to understand a library's API — use this instead of calling list_symbols repeatedly. Filter by module_path (e.g. 'runtime', 'runtime::task') to focus on a specific module. Supports pagination via limit/offset for very large crates.")]
    async fn get_public_api(
        &self,
        params: Parameters<GetPublicApiParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_public_api(snapshot_id, p.module_path.as_deref(), p.limit, p.offset).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the hierarchical module tree of a crate. Returns a tree of modules with: name, path (e.g. 'runtime::task'), doc comment, number of public items, and optionally the signatures of all public symbols in each module. This is the best way to understand the structure and organization of a Rust crate. Use depth=1 or depth=2 for a top-level overview, then drill into specific modules with get_public_api(module_path=...). Set include_signatures=true to also get the symbol signatures inline (useful for small modules).")]
    async fn get_module_tree(
        &self,
        params: Parameters<GetModuleTreeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_module_tree(snapshot_id, p.depth, p.include_signatures).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get EVERYTHING about a type in a SINGLE call: its definition (signature, doc comment, full body), methods, fields/variants, traits it implements, types that implement it (if it's a trait), functions that call/use it, and types it depends on. This replaces the need to call get_symbol + find_references + get_dependencies + get_implementations separately. Use this when you need to fully understand a struct, enum, trait, or class.")]
    async fn get_type_context(
        &self,
        params: Parameters<GetTypeContextParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_type_context(snapshot_id, p.symbol_id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Extract code examples from a symbol's doc comments. Parses fenced code blocks (```rust ... ```) from the doc comment and returns them as structured examples with language tag and code content. Great for understanding how to use a function, struct, or trait by its documentation examples.")]
    async fn get_examples(
        &self,
        params: Parameters<GetExamplesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_examples(snapshot_id, p.symbol_id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Generate an LLM summary for a crate, module, or type. This TRIGGERS generation (costs LLM tokens) — use get_summary to retrieve previously generated summaries. Scope values: 'crate' (whole crate overview), 'module:<path>' (e.g. 'module:runtime'), 'type:<symbol_id>' (e.g. 'type:42'). Requires an LLM provider configured on the server (GITDOC_LLM_ENDPOINT). Returns the generated summary text.")]
    async fn summarize(
        &self,
        params: Parameters<SummarizeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.summarize(snapshot_id, &p.scope).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Retrieve a previously generated LLM summary. Use 'summarize' to generate summaries first. Scope values: 'crate', 'module:<path>', 'type:<symbol_id>'. Omit scope to list all available summaries for the snapshot.")]
    async fn get_summary(
        &self,
        params: Parameters<GetSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.get_summary(snapshot_id, p.scope.as_deref()).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Ask a natural language question about a codebase and get an assembled answer with relevant symbols, docs, and relationships. Uses semantic search to find relevant content, then enriches each hit with type context (methods, traits, dependencies). Set synthesize=true to get an LLM-generated answer on top of the assembled context. This is the highest-level exploration tool — use it when you have a conceptual question like 'how does task scheduling work?' or 'what's the error handling strategy?'. Requires embedding provider; optionally requires LLM provider for synthesis.")]
    async fn explain(
        &self,
        params: Parameters<ExplainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.explain(snapshot_id, &p.query, p.synthesize, p.limit).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Semantic search by meaning across docs and code using vector embeddings. Use natural language queries like 'how is authentication handled?' or 'error retry logic' to find relevant content even without exact keyword matches. Scope: 'all' (default), 'docs' (only documentation), or 'symbols' (only code). Requires an embedding provider configured on the server (COHERE_KEY or OPENAI_API_KEY). Returns error if no provider is available — fall back to search_docs or search_symbols instead.")]
    async fn semantic_search(
        &self,
        params: Parameters<SemanticSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.semantic_search(snapshot_id, &p.query, p.scope.as_deref(), p.limit).await {
            Ok(results) => json_result(&results),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Ask a question about a codebase in conversational mode. Maintains a persistent conversation per repo — follow-up questions automatically have context from previous turns. Uses semantic search + LLM to produce a synthesized answer in a SINGLE call. This is the PREFERRED tool for exploring a codebase: just ask questions naturally ('What does this crate do?', 'How is error handling done?', 'Show me the main entry point') and the conversation builds context over time. Requires a snapshot (call register_repo then index_repo first if not done yet). Set detail_level='with_source' for verbatim source code in answers. Requires both embedding provider and LLM provider on the server.")]
    async fn ask(
        &self,
        params: Parameters<AskParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };

        // Auto-generate cheatsheet if missing
        match self.client.get_cheatsheet(&p.repo_id).await {
            Ok(cs) if cs.get("content").and_then(|v| v.as_str()).map_or(true, |s| s.is_empty()) => {
                self.auto_generate_cheatsheet(&p.repo_id, snapshot_id).await;
            }
            Err(_) => {
                self.auto_generate_cheatsheet(&p.repo_id, snapshot_id).await;
            }
            _ => {} // cheatsheet exists
        }

        // Look up or create conversation for this repo
        let conversation_id = {
            let map = self.conversations.lock().await;
            map.get(&p.repo_id).and_then(|(sid, cid)| {
                if *sid == snapshot_id { Some(*cid) } else { None }
            })
        };

        let detail_level = p.detail_level.as_ref().map(|dl| match dl {
            DetailLevel::Brief => "brief",
            DetailLevel::Detailed => "detailed",
            DetailLevel::WithSource => "with_source",
        });
        match self.client.converse(snapshot_id, &p.question, conversation_id, p.limit, detail_level).await {
            Ok(resp) => {
                // Store the conversation_id for future calls
                {
                    let mut map = self.conversations.lock().await;
                    map.insert(p.repo_id.clone(), (snapshot_id, resp.conversation_id));
                }

                // Format response
                let mut output = resp.answer.clone();
                if !resp.sources.is_empty() {
                    output.push_str("\n\n---\n**Sources:**\n");
                    for src in &resp.sources {
                        if let Some(sid) = src.symbol_id {
                            output.push_str(&format!("- [{}] {} ({}) — symbol_id: {}\n", src.kind, src.name, src.file_path, sid));
                        } else {
                            output.push_str(&format!("- [{}] {} ({})\n", src.kind, src.name, src.file_path));
                        }
                    }
                }
                text_result(output)
            }
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the persistent repo cheatsheet — a structured summary of architecture, key types, patterns, and gotchas accumulated over time. Returns the current cheatsheet content. If no cheatsheet exists yet, use update_cheatsheet to generate one.")]
    async fn get_cheatsheet(
        &self,
        params: Parameters<GetCheatsheetParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match self.client.get_cheatsheet(&p.repo_id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Generate or regenerate the persistent repo cheatsheet. This triggers LLM generation (costs tokens) based on the repo's module tree, public API, README, and existing summaries. The cheatsheet is automatically injected into 'ask' conversations to provide context. Use this after initial indexing or when the repo has changed significantly.")]
    async fn update_cheatsheet(
        &self,
        params: Parameters<UpdateCheatsheetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };
        match self.client.generate_cheatsheet(&p.repo_id, snapshot_id, Some("manual")).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List cheatsheet patch history for a repo — shows how the cheatsheet evolved over time. Each patch records the change summary, trigger (manual/auto/conversation_reset), and timestamp. Use get_cheatsheet_patch to see the full diff (prev_content vs new_content) of a specific patch.")]
    async fn list_cheatsheet_patches(
        &self,
        params: Parameters<ListCheatsheetPatchesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.list_cheatsheet_patches(&p.repo_id, p.limit, p.offset).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get a specific cheatsheet patch by ID — shows the full before/after content (prev_content, new_content), change summary, trigger, and model used. Useful for understanding what changed in the cheatsheet and why.")]
    async fn get_cheatsheet_patch(
        &self,
        params: Parameters<GetCheatsheetPatchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.get_cheatsheet_patch(&p.repo_id, p.patch_id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    // --- Architect tools ---

    #[tool(description = "List all library profiles in the Architect knowledge base. Returns id, name, category, version_hint, source, and last update time. Filter by category (e.g. 'web-framework', 'database') to narrow results.")]
    async fn list_lib_profiles(
        &self,
        params: Parameters<ListLibProfilesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.list_lib_profiles(p.category.as_deref()).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get the full profile of a library from the Architect knowledge base. Returns detailed information including what it is, key APIs, strengths, limitations, gotchas, and ecosystem fit.")]
    async fn get_lib_profile(
        &self,
        params: Parameters<GetLibProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.get_lib_profile(&p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Ingest a library from its git URL into the Architect knowledge base. This clones the repo, indexes it, and uses LLM to generate a structured profile. Use this to add new libraries to the knowledge base for future architecture recommendations.")]
    async fn ingest_lib(
        &self,
        params: Parameters<IngestLibParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;

        // Step 1: Register repo if not already registered
        self.log_info(&format!("Ingesting library '{}' from {}...", p.name, p.git_url)).await;
        let repo_id = format!("lib-{}", p.id);
        match self.client.create_repo(&repo_id, &p.name, &p.git_url).await {
            Ok(_) => self.log_info("Repository registered").await,
            Err(e) => {
                let err_str = e.to_string();
                if !err_str.contains("already exists") && !err_str.contains("409") {
                    return err_result(format!("error registering repo: {e}"));
                }
                self.log_info("Repository already registered").await;
            }
        }

        // Step 2: Index repo
        self.log_info("Indexing repository...").await;
        let index_result = match self.client.index_repo(&repo_id, "HEAD", Some("latest"), true).await {
            Ok(r) => r,
            Err(e) => return err_result(format!("error indexing repo: {e}")),
        };
        self.log_info(&format!("Indexed: {} symbols", index_result.symbols_count)).await;

        // Step 3: Create initial profile entry then generate
        let body = requests::CreateLibRequest {
            id: p.id.clone(),
            name: p.name,
            category: p.category,
            version_hint: p.version_hint,
            profile: None,
        };
        let _ = self.client.create_lib_profile(&body).await;

        // Step 4: Generate profile from indexed repo
        self.log_info("Generating library profile with LLM...").await;
        let gen_body = requests::GenerateLibProfileRequest {
            repo_id,
            snapshot_id: Some(index_result.snapshot_id),
        };
        match self.client.generate_lib_profile(&p.id, &gen_body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error generating profile: {e}")),
        }
    }

    #[tool(description = "Import a library profile manually into the Architect knowledge base. Use this when the library isn't available as a git repo, or when you want to provide a custom profile. The profile should be structured markdown text.")]
    async fn import_lib_profile(
        &self,
        params: Parameters<ImportLibProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::CreateLibRequest {
            id: p.id,
            name: p.name,
            category: p.category,
            version_hint: p.version_hint,
            profile: Some(p.profile),
        };
        match self.client.create_lib_profile(&body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Regenerate the profile of a library that is already indexed. Use this after re-indexing a repo to update the profile with the latest code analysis.")]
    async fn generate_lib_profile(
        &self,
        params: Parameters<GenerateLibProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::GenerateLibProfileRequest {
            repo_id: p.repo_id,
            snapshot_id: p.snapshot_id,
        };
        match self.client.generate_lib_profile(&p.id, &body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Delete a library profile from the Architect knowledge base.")]
    async fn delete_lib_profile(
        &self,
        params: Parameters<DeleteLibProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.delete_lib_profile(&p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Add a global stack rule to the Architect knowledge base. Stack rules encode technology preferences, constraints, and guidelines (e.g. 'prefer Axum over Actix-web for new Rust services', 'always use connection pooling for databases'). Rules are used by the Architect to inform recommendations.")]
    async fn add_stack_rule(
        &self,
        params: Parameters<AddStackRuleParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::UpsertRuleRequest {
            id: None,
            rule_type: p.rule_type,
            subject: p.subject,
            content: p.content,
            lib_profile_id: p.lib_profile_id,
            priority: p.priority,
        };
        match self.client.upsert_stack_rule(&body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all stack rules in the Architect knowledge base. Filter by rule_type (e.g. 'prefer', 'avoid', 'guideline') or subject (e.g. 'HTTP framework', 'database').")]
    async fn list_stack_rules(
        &self,
        params: Parameters<ListStackRulesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.list_stack_rules(p.rule_type.as_deref(), p.subject.as_deref()).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Delete a stack rule from the Architect knowledge base.")]
    async fn delete_stack_rule(
        &self,
        params: Parameters<DeleteStackRuleParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.delete_stack_rule(p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Ask the Architect for technology advice. Provide a natural language question about technology choices, library selection, or architecture decisions. The Architect searches its knowledge base of library profiles and stack rules, then uses LLM to synthesize an informed recommendation. Example questions: 'What HTTP framework should I use for Rust?', 'How should I handle database migrations?', 'What's the best approach for async task scheduling?'")]
    async fn architect_advise(
        &self,
        params: Parameters<ArchitectAdviseParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let body = requests::AdviseRequest {
            question: p.question,
            limit: p.limit,
        };
        match self.client.architect_advise(&body).await {
            Ok(result) => {
                // Format response nicely
                let answer = result.get("answer").and_then(|v| v.as_str()).unwrap_or("");
                let mut output = answer.to_string();

                if let Some(libs) = result.get("relevant_libs").and_then(|v| v.as_array()) {
                    if !libs.is_empty() {
                        output.push_str("\n\n---\n**Relevant library profiles:**\n");
                        for lib in libs {
                            let id = lib.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let score = lib.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            output.push_str(&format!("- {} (relevance: {:.2})\n", id, score));
                        }
                    }
                }
                if let Some(rules) = result.get("relevant_rules").and_then(|v| v.as_array()) {
                    if !rules.is_empty() {
                        output.push_str("\n**Relevant stack rules:**\n");
                        for rule in rules {
                            let id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let score = rule.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            output.push_str(&format!("- Rule #{} (relevance: {:.2})\n", id, score));
                        }
                    }
                }
                text_result(output)
            }
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Reset the conversation for a repository. Clears all conversation history so the next 'ask' call starts fresh. Use this when switching topics or when the conversation context has become stale or irrelevant.")]
    async fn conversation_reset(
        &self,
        params: Parameters<ConversationResetParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let snapshot_id = match self.resolve_snapshot_id(&p.repo_id, p.reference.as_deref()).await {
            Ok(id) => id,
            Err(e) => return err_result(e),
        };

        let conversation_id = {
            let mut map = self.conversations.lock().await;
            map.remove(&p.repo_id).and_then(|(sid, cid)| {
                if sid == snapshot_id { Some(cid) } else { None }
            })
        };

        if let Some(cid) = conversation_id {
            match self.client.delete_conversation(snapshot_id, cid).await {
                Ok(_) => text_result("Conversation reset. Next 'ask' call will start a new conversation.".into()),
                Err(e) => err_result(format!("error deleting conversation: {e}")),
            }
        } else {
            text_result("No active conversation for this repo.".into())
        }
    }

    // --- Project Profiles ---

    #[tool(description = "Create or update a project profile. Defines the technology stack, constraints, and code style for a project. This context is automatically used by architect_advise to provide project-aware recommendations.")]
    async fn create_project_profile(
        &self,
        params: Parameters<CreateProjectProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::CreateProjectProfileRequest {
            id: p.id,
            repo_id: p.repo_id,
            name: p.name,
            description: p.description,
            stack: p.stack,
            constraints: p.constraints,
            code_style: p.code_style,
        };
        match self.client.create_project_profile(&body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get a project profile by ID. Returns the full project definition including stack, constraints, and code style.")]
    async fn get_project_profile(
        &self,
        params: Parameters<GetProjectProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.get_project_profile(&p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List all project profiles in the Architect knowledge base.")]
    async fn list_project_profiles(
        &self,
        _params: Parameters<ListProjectProfilesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        match self.client.list_project_profiles().await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Delete a project profile from the Architect knowledge base.")]
    async fn delete_project_profile(
        &self,
        params: Parameters<DeleteProjectProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.delete_project_profile(&p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    // --- Architecture Decisions ---

    #[tool(description = "Record an architecture decision. Captures what was decided, why, what alternatives were considered, and optionally links to a project profile. Decisions are searchable by architect_advise and help inform future recommendations. Use status 'active' (default), 'superseded', or 'reverted'.")]
    async fn record_decision(
        &self,
        params: Parameters<RecordDecisionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::CreateDecisionRequest {
            project_profile_id: p.project_profile_id,
            title: p.title,
            context: p.context,
            choice: p.choice,
            alternatives: p.alternatives,
            reasoning: p.reasoning,
        };
        match self.client.create_decision(&body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List architecture decisions. Filter by project_profile_id or status ('active', 'superseded', 'reverted').")]
    async fn list_decisions(
        &self,
        params: Parameters<ListDecisionsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.list_decisions(p.project_profile_id.as_deref(), p.status.as_deref()).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Update an architecture decision's outcome or status. Use this to record what actually happened after a decision was made, or to mark it as 'superseded' or 'reverted'.")]
    async fn update_decision(
        &self,
        params: Parameters<UpdateDecisionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::UpdateDecisionRequest {
            outcome: p.outcome,
            status: p.status,
        };
        match self.client.update_decision(p.id, &body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    // --- Compare Libs ---

    #[tool(description = "Compare libraries side-by-side. Provide library profile IDs and evaluation criteria. Returns a structured comparison with fit scores, pros/cons, differentiators, and a recommendation. Example: compare_libs(['axum', 'actix-web'], 'building a REST API with WebSocket support')")]
    async fn compare_libs(
        &self,
        params: Parameters<CompareLibsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let body = requests::CompareLibsRequest {
            lib_ids: p.lib_ids,
            criteria: p.criteria,
        };
        match self.client.compare_libs(&body).await {
            Ok(result) => {
                let comparison = result.get("comparison").and_then(|v| v.as_str()).unwrap_or("");
                text_result(comparison.to_string())
            }
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    // --- Architecture Patterns ---

    #[tool(description = "Add an architecture pattern to the knowledge base. Patterns describe HOW to use libraries together (e.g. 'JWT auth with axum + tower'). Include code examples, steps, and best practices. Patterns are surfaced by architect_advise when relevant.")]
    async fn add_pattern(
        &self,
        params: Parameters<AddPatternParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        let body = requests::CreatePatternRequest {
            name: p.name,
            category: p.category,
            description: p.description,
            libs_involved: p.libs_involved,
            pattern_text: p.pattern_text,
        };
        match self.client.create_pattern(&body).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "List architecture patterns in the knowledge base. Optionally filter by category.")]
    async fn list_patterns(
        &self,
        params: Parameters<ListPatternsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.list_patterns(p.category.as_deref()).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Get a specific architecture pattern by ID.")]
    async fn get_pattern(
        &self,
        params: Parameters<GetPatternParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.get_pattern(p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }

    #[tool(description = "Delete an architecture pattern from the knowledge base.")]
    async fn delete_pattern(
        &self,
        params: Parameters<DeletePatternParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_granular()?;
        let p = params.0;
        match self.client.delete_pattern(p.id).await {
            Ok(result) => json_result(&result),
            Err(e) => err_result(format!("error: {e}")),
        }
    }
}

#[tool_handler]
impl ServerHandler for GitdocMcpServer {
    fn get_info(&self) -> ServerInfo {
        let instructions = if self.mode_filter.is_granular() {
            GRANULAR_INSTRUCTIONS
        } else {
            SIMPLE_INSTRUCTIONS
        };
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .build(),
            server_info: Implementation {
                name: "gitdoc-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(instructions.into()),
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        *self.peer.write().await = Some(context.peer);
        Ok(self.get_info())
    }
}

