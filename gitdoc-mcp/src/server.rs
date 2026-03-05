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

use crate::client::GitdocClient;
use crate::config::McpMode;
use crate::instructions::{SIMPLE_INSTRUCTIONS, GRANULAR_INSTRUCTIONS};
use crate::params::*;
use crate::snapshot_resolver::resolve_snapshot;

/// Per-repo conversation state: (snapshot_id, conversation_id)
type ConversationMap = Arc<Mutex<HashMap<String, (i64, i64)>>>;

#[derive(Clone)]
pub struct GitdocMcpServer {
    tool_router: ToolRouter<Self>,
    client: Arc<GitdocClient>,
    conversations: ConversationMap,
    mode: McpMode,
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
    pub fn new(client: GitdocClient, mode: McpMode) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Arc::new(client),
            conversations: Arc::new(Mutex::new(HashMap::new())),
            mode,
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
        if self.mode == McpMode::Simple {
            return Err(McpError::invalid_request(
                "This tool is not available in simple mode. Use GITDOC_MCP_MODE=granular to enable all tools.",
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

    #[tool(description = "Ask a question about a codebase in conversational mode. Maintains a persistent conversation per repo — follow-up questions automatically have context from previous turns. Uses semantic search + LLM to produce a synthesized answer in a SINGLE call. This is the PREFERRED tool for exploring a codebase: just ask questions naturally ('What does this crate do?', 'How is error handling done?', 'Show me the main entry point') and the conversation builds context over time. Requires both embedding provider and LLM provider on the server.")]
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

        match self.client.converse(snapshot_id, &p.question, conversation_id, p.limit).await {
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
        self.check_granular()?;
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
        let body = serde_json::json!({
            "id": p.id,
            "name": p.name,
            "category": p.category.unwrap_or_default(),
            "version_hint": p.version_hint.unwrap_or_default(),
        });
        let _ = self.client.create_lib_profile(&body).await;

        // Step 4: Generate profile from indexed repo
        self.log_info("Generating library profile with LLM...").await;
        let gen_body = serde_json::json!({
            "repo_id": repo_id,
            "snapshot_id": index_result.snapshot_id,
        });
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
        let body = serde_json::json!({
            "id": p.id,
            "name": p.name,
            "category": p.category.unwrap_or_default(),
            "version_hint": p.version_hint.unwrap_or_default(),
            "profile": p.profile,
        });
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
        let body = serde_json::json!({
            "repo_id": p.repo_id,
            "snapshot_id": p.snapshot_id,
        });
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
        let body = serde_json::json!({
            "rule_type": p.rule_type,
            "subject": p.subject,
            "content": p.content,
            "lib_profile_id": p.lib_profile_id,
            "priority": p.priority.unwrap_or(0),
        });
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
        let body = serde_json::json!({
            "question": p.question,
            "limit": p.limit.unwrap_or(5),
        });
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
        let body = serde_json::json!({
            "id": p.id,
            "repo_id": p.repo_id,
            "name": p.name,
            "description": p.description.unwrap_or_default(),
            "stack": p.stack.unwrap_or(serde_json::json!([])),
            "constraints": p.constraints.unwrap_or_default(),
            "code_style": p.code_style.unwrap_or_default(),
        });
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
        let body = serde_json::json!({
            "project_profile_id": p.project_profile_id,
            "title": p.title,
            "context": p.context.unwrap_or_default(),
            "choice": p.choice,
            "alternatives": p.alternatives.unwrap_or_default(),
            "reasoning": p.reasoning.unwrap_or_default(),
        });
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
        let body = serde_json::json!({
            "outcome": p.outcome,
            "status": p.status,
        });
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
        let body = serde_json::json!({
            "lib_ids": p.lib_ids,
            "criteria": p.criteria,
        });
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
        let body = serde_json::json!({
            "name": p.name,
            "category": p.category.unwrap_or_default(),
            "description": p.description.unwrap_or_default(),
            "libs_involved": p.libs_involved.unwrap_or_default(),
            "pattern_text": p.pattern_text,
        });
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
        let instructions = match self.mode {
            McpMode::Simple => SIMPLE_INSTRUCTIONS,
            McpMode::Granular => GRANULAR_INSTRUCTIONS,
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

2. If not: `register_repo(id: "mylib", name: "My Lib", url: "https://github.com/...")` → server clones it
3. `index_repo(repo_id: "mylib")` → creates a searchable snapshot. **Required before querying.**
4. `ask(repo_id: "mylib", question: "What does this crate do?")` → get answers with sources

## Workflow

- **`ask`** is the main tool — ask any question and get an LLM-synthesized answer with source references
- Follow-up questions keep conversation context: `ask(repo_id: "mylib", question: "How does error handling work?")`
- Use `conversation_reset` when switching to an unrelated topic
- Use `get_repo_overview` for a quick snapshot of README + structure

## Tools Available

| Tool | Purpose |
|------|---------|
| `ping` | Health check |
| `list_repos` | Discover registered repos |
| `register_repo` | Add a new repo (server clones it) |
| `index_repo` | Create a snapshot (required before querying) |
| `get_repo_overview` | README + doc listing + top symbols |
| `ask` | Ask questions — conversational, context-aware |
| `conversation_reset` | Clear conversation when switching topics |
| `architect_advise` | Ask for technology/architecture recommendations based on a knowledge base of library profiles, stack rules, project profiles, decisions, and patterns |
| `compare_libs` | Compare libraries side-by-side with structured fit scores, pros/cons, and recommendation |

## Tips

- Do NOT clone repos yourself — just pass the URL to `register_repo`
- Set `fetch=true` on `index_repo` to pull latest changes before indexing
- If `ask` returns errors about embeddings, ensure the server has COHERE_KEY or OPENAI_API_KEY configured"#;

const GRANULAR_INSTRUCTIONS: &str = r#"# GitDoc MCP — Code Intelligence for LLM Agents

GitDoc indexes git repositories and exposes their documentation, code symbols, and cross-references as structured data. You NEVER read raw source files — instead, you navigate through extracted symbols, docs, and a dependency graph.

Supported languages: Rust (.rs), TypeScript (.ts/.tsx), JavaScript (.js/.jsx), Markdown (.md/.mdx).

## Quick Start — MANDATORY steps before querying

You MUST follow these steps for any new repository:

1. `list_repos` → check if the repo is already registered
2. If not registered: `register_repo` with the git clone URL — the server clones and manages the repo
3. `index_repo` → creates a snapshot. **Nothing works without at least one snapshot.**
4. Now you can query: `get_repo_overview`, `list_symbols`, `search_symbols`, etc.

Example — register and index a repo:
  register_repo(id: "tokio", name: "Tokio", url: "https://github.com/tokio-rs/tokio.git")
  index_repo(repo_id: "tokio", label: "latest")
  get_repo_overview(repo_id: "tokio", ref: "latest")

IMPORTANT: Do NOT clone repositories yourself. The server handles all git cloning internally.

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
| `register_repo` | Add a new repo (server clones it) | `id`, `name`, `url` |
| `index_repo` | Create a snapshot (REQUIRED before querying) | `repo_id`, optional: `commit`, `label`, `fetch` |
| `fetch_repo` | Update a URL-cloned repo (does NOT re-index) | `repo_id` |

### High-Level Views (START HERE for complex libraries — 2-3 calls to understand a whole crate)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_module_tree` | **Best starting point for Rust crates.** See the full module hierarchy | Tree of modules with doc comments and item counts |
| `get_public_api` | **Get a crate's complete API cheat sheet** in one call | All public signatures grouped by module, with impl methods merged onto types |
| `get_type_context` | **Understand a type completely** in one call | Definition + methods + traits + implementors + callers + dependencies |
| `get_examples` | See how a symbol is used | Code examples extracted from doc comments |

### Browsing (detailed exploration)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_repo_overview` | Read README and see structure | README content, doc file listing, top-level public symbols |
| `list_docs` | Browse documentation files | File paths and titles |
| `read_doc` | Read a specific doc file | Full text content with title |
| `list_symbols` | Browse code symbols (use get_public_api instead for API overview) | name, kind, visibility, signature, file_path, line numbers, doc_comment |
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
| `explain` | **Ask a question in natural language** — assembles context from semantic search + type context | Relevant symbols with methods/traits, docs, optional LLM synthesis |
| `search_docs` | Find docs by keyword | Matching docs with highlighted snippets |
| `search_symbols` | Find symbols by keyword (name, signature, doc comment) | Matching symbols with relevance score |
| `semantic_search` | Find by meaning ("how is auth handled?") | Docs and/or symbols ranked by semantic similarity |

### LLM Summaries (requires GITDOC_LLM_ENDPOINT configured)
| Tool | When to use | Returns |
|------|-------------|---------|
| `summarize` | **Generate** an LLM summary (costs tokens) | Generated summary for crate/module/type |
| `get_summary` | **Retrieve** a previously generated summary | Cached summary or list of available summaries |

### Cheatsheet (persistent repo knowledge)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_cheatsheet` | **Read the repo cheatsheet** — architecture, key types, patterns, gotchas | Current cheatsheet content |
| `update_cheatsheet` | **Generate/regenerate** the cheatsheet (costs LLM tokens) | Generated cheatsheet with patch ID |

### Conversational Mode (RECOMMENDED — fewest tool calls)
| Tool | When to use | Returns |
|------|-------------|---------|
| `ask` | **Ask any question about the codebase** — maintains conversation context across calls, auto-injects cheatsheet | LLM-synthesized answer with source references |
| `conversation_reset` | Clear conversation history for a repo to start fresh (auto-updates cheatsheet with learnings) | Confirmation message |

### Architect (Technology Knowledge Base)
| Tool | When to use | Returns |
|------|-------------|---------|
| `architect_advise` | **Ask for technology recommendations** — searches lib profiles, stack rules, project profiles, decisions, patterns, and cheatsheets | LLM-synthesized recommendation |
| `compare_libs` | **Compare libraries side-by-side** — structured comparison with fit scores, pros/cons, recommendation | Structured comparison |
| `list_lib_profiles` | Browse available library profiles in the knowledge base | List of profiles with id, name, category |
| `get_lib_profile` | Get full profile of a library (what it is, key APIs, strengths, limitations, gotchas) | Complete profile text |
| `ingest_lib` | Add a library to the knowledge base from its git URL (clone + index + LLM profile) | Generated profile |
| `import_lib_profile` | Manually import a library profile (markdown text) | Stored profile |
| `generate_lib_profile` | Regenerate profile for an already-indexed library | Updated profile |
| `delete_lib_profile` | Remove a library profile | Confirmation |
| `add_stack_rule` | Add a global stack rule (e.g. "prefer Axum over Actix-web") | Stored rule |
| `list_stack_rules` | Browse stack rules (filter by type or subject) | List of rules |
| `delete_stack_rule` | Remove a stack rule | Confirmation |
| `create_project_profile` | Define a project's stack, constraints, and code style | Stored profile |
| `get_project_profile` | Get a project profile | Full project definition |
| `list_project_profiles` | List all project profiles | Summary list |
| `delete_project_profile` | Remove a project profile | Confirmation |
| `record_decision` | Record an architecture decision (title, choice, reasoning, alternatives) | Stored decision |
| `list_decisions` | List decisions (filter by project, status) | Decision list |
| `update_decision` | Update a decision's outcome or status (active/superseded/reverted) | Updated decision |
| `add_pattern` | Add an architecture pattern (e.g. "JWT auth with axum + tower") | Stored pattern |
| `list_patterns` | List patterns (filter by category) | Pattern list |
| `get_pattern` | Get a specific pattern | Full pattern with code examples |
| `delete_pattern` | Remove a pattern | Confirmation |

### Maintenance
| Tool | When to use |
|------|-------------|
| `diff_symbols` | Compare two snapshots — see added/removed/modified symbols |

## Recommended Exploration Workflow

### Conversational mode (PREFERRED — minimum tool calls):
1. `ask(repo_id: "X", question: "What does this crate do?")` → get an overview
2. `ask(repo_id: "X", question: "How does error handling work?")` → follow-up with context
3. `ask(repo_id: "X", question: "Show me the main types")` → keeps building on prior answers
4. `conversation_reset(repo_id: "X")` → only when switching to unrelated topic

### For understanding a complex library (Rust crate with many modules):
1. `get_module_tree(repo_id: "X", depth: 2)` → see the module hierarchy
2. `get_public_api(repo_id: "X", module_path: "runtime")` → get all public signatures in a module
3. `get_type_context(symbol_id: 123, repo_id: "X")` → deep-dive into a specific type
4. `get_examples(symbol_id: 123, repo_id: "X")` → see usage examples from doc comments

### For general exploration:
1. `list_repos` → find available repos
2. `get_repo_overview(repo_id: "X")` → read README, see doc tree and top symbols
3. `search_symbols(repo_id: "X", query: "what you're looking for")` → find relevant symbols
4. `get_symbol(symbol_id: 123)` → read the full implementation
5. `find_references(symbol_id: 123, repo_id: "X")` → see who calls it
6. `get_dependencies(symbol_id: 123, repo_id: "X")` → see what it depends on

## Common Pitfalls

- **Do NOT clone repos yourself**: The server handles all git cloning. Just pass the URL to `register_repo`.
- **"No snapshot found"**: You forgot to call `index_repo` first. Every repo must be indexed before querying.
- **"error resolving snapshot"**: The `ref` value doesn't match any label or commit SHA. Use `list_repos` to see available snapshots with their labels and commits.
- **semantic_search returns 503**: No embedding provider configured on the server. Use `search_docs` or `search_symbols` instead.
- **symbol_id is a number**: Don't pass a string. It's an integer returned by list/search tools.

## Symbol Kinds

Valid values for the `kind` filter: function, struct, class, trait, interface, enum, type_alias, const, static, module, macro.

## Reference Kinds

Valid values for the `kind` filter on find_references/get_dependencies: calls, type_ref, implements, imports."#;
