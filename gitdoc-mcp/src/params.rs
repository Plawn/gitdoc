use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct RegisterRepoParams {
    /// A unique identifier for the repository (e.g. "my-project", "tokio", "react")
    pub id: String,
    /// Git clone URL (e.g. "https://github.com/user/repo.git"). The server clones and manages the directory.
    pub url: String,
    /// Human-readable name for the repository (e.g. "My Project")
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FetchRepoParams {
    /// The repo ID to fetch latest changes for
    pub repo_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct IndexRepoParams {
    /// The repo ID to index
    pub repo_id: String,
    /// Git commit/ref to index (default: HEAD)
    pub commit: Option<String>,
    /// Optional human-readable label for the snapshot (e.g. "v1.0", "before-refactor"). Used later to reference this snapshot via the 'ref' parameter.
    pub label: Option<String>,
    /// If true, fetch latest changes from the remote before indexing. Default: false.
    pub fetch: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RepoRefParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadDocParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// File path of the doc to read (e.g. "README.md")
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListSymbolsParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Filter by file path
    pub file_path: Option<String>,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    pub kind: Option<String>,
    /// Include private symbols (default: false)
    pub include_private: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetSymbolParams {
    /// The symbol ID (globally unique)
    pub symbol_id: i64,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    /// The symbol ID to find references for
    pub symbol_id: i64,
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Filter by ref kind (e.g. "calls", "type_ref", "implements")
    pub kind: Option<String>,
    /// Maximum number of results (default: 20)
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetDependenciesParams {
    /// The symbol ID to get dependencies for
    pub symbol_id: i64,
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Filter by ref kind (e.g. "calls", "type_ref")
    pub kind: Option<String>,
    /// Maximum number of results (default: 20)
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetImplementationsParams {
    /// The symbol ID (trait, interface, or class)
    pub symbol_id: i64,
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DiffSymbolsParams {
    /// The repo ID
    pub repo_id: String,
    /// Optional ref for the 'from' snapshot: label, SHA prefix, or omit for latest
    pub from_ref: Option<String>,
    /// Optional ref for the 'to' snapshot: label, SHA prefix, or omit for latest
    pub to_ref: Option<String>,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    pub kind: Option<String>,
    /// Include private symbols (default: false)
    pub include_private: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchDocsParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Search query text
    pub query: String,
    /// Maximum number of results (default: 10)
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchSymbolsParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Search query text
    pub query: String,
    /// Filter by symbol kind (e.g. "function", "struct", "class")
    pub kind: Option<String>,
    /// Filter by visibility (e.g. "pub", "private")
    pub visibility: Option<String>,
    /// Maximum number of results (default: 10)
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetPublicApiParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Filter by module path (e.g. "runtime", "runtime::task"). Uses Rust module path syntax with "::" separators.
    pub module_path: Option<String>,
    /// Maximum number of symbols to return (default: 2000)
    pub limit: Option<i64>,
    /// Offset for pagination (default: 0)
    pub offset: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetModuleTreeParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Maximum depth of the tree to return (default: unlimited). Use 1-2 to get a top-level overview.
    pub depth: Option<usize>,
    /// If true, include public symbol signatures in each module node (default: false)
    pub include_signatures: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetTypeContextParams {
    /// The symbol ID of the type/trait/enum to get context for
    pub symbol_id: i64,
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetExamplesParams {
    /// The symbol ID to extract examples from
    pub symbol_id: i64,
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SummarizeParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Summary scope: "crate" for whole-crate summary, "module:<path>" (e.g. "module:runtime") for a module, or "type:<symbol_id>" (e.g. "type:42") for a specific type
    pub scope: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetSummaryParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Summary scope: "crate", "module:<path>", or "type:<symbol_id>". Omit to list all available summaries.
    pub scope: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExplainParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Natural language question (e.g. "how to create a TCP server?", "what is the lifecycle of a task?")
    pub query: String,
    /// If true, use LLM to synthesize a final answer from the assembled context (requires GITDOC_LLM_ENDPOINT). Default: false.
    pub synthesize: Option<bool>,
    /// Maximum number of initial semantic search hits (default: 10)
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SemanticSearchParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Natural language query (e.g. "how is authentication handled?")
    pub query: String,
    /// Search scope: "all", "docs", or "symbols" (default: "all")
    pub scope: Option<String>,
    /// Maximum number of results (default: 10)
    pub limit: Option<usize>,
}
