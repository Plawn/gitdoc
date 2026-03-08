use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct SetModeParams {
    /// Tool mode: 'simple' (conversational, fewer tools) or 'granular' (full control, all tools).
    /// Use 'granular' when you need exact source code, symbol definitions, or fine-grained navigation.
    pub mode: SetModeValue,
}

#[derive(Deserialize, JsonSchema)]
pub enum SetModeValue {
    #[serde(rename = "simple")]
    Simple,
    #[serde(rename = "granular")]
    Granular,
}

/// Level of detail for ask responses
#[derive(Deserialize, JsonSchema)]
pub enum DetailLevel {
    /// Concise answers
    #[serde(rename = "brief")]
    Brief,
    /// Thorough analysis (default)
    #[serde(rename = "detailed")]
    Detailed,
    /// Include verbatim source code from the index
    #[serde(rename = "with_source")]
    WithSource,
}

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
pub struct AskParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Natural language question about the codebase
    pub question: String,
    /// Maximum number of semantic search hits for context (default: 8)
    pub limit: Option<usize>,
    /// Level of detail in the answer
    pub detail_level: Option<DetailLevel>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ConversationResetParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetCheatsheetParams {
    /// The repo ID
    pub repo_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateCheatsheetParams {
    /// The repo ID
    pub repo_id: String,
    /// Snapshot reference: a label (e.g. "v1.0"), a commit SHA prefix (e.g. "abc123"), or omit to use the latest snapshot
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListCheatsheetPatchesParams {
    /// The repo ID
    pub repo_id: String,
    /// Maximum number of patches to return (default: 20)
    pub limit: Option<i64>,
    /// Offset for pagination (default: 0)
    pub offset: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetCheatsheetPatchParams {
    /// The repo ID
    pub repo_id: String,
    /// The patch ID to retrieve
    pub patch_id: i64,
}

// --- Architect params ---

#[derive(Deserialize, JsonSchema)]
pub struct ListLibProfilesParams {
    /// Filter by category (e.g. "web-framework", "database")
    pub category: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetLibProfileParams {
    /// The lib profile ID (e.g. "axum", "tokio")
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct IngestLibParams {
    /// Unique ID for the library (e.g. "axum")
    pub id: String,
    /// Human-readable name (e.g. "Axum")
    pub name: String,
    /// Git clone URL (e.g. "https://github.com/tokio-rs/axum.git")
    pub git_url: String,
    /// Library category (e.g. "web-framework", "database")
    pub category: Option<String>,
    /// Version hint (e.g. "0.7", "1.x")
    pub version_hint: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportLibProfileParams {
    /// Unique ID for the library
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Library category
    pub category: Option<String>,
    /// Version hint
    pub version_hint: Option<String>,
    /// The profile text (markdown)
    pub profile: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GenerateLibProfileParams {
    /// The lib profile ID to generate/regenerate
    pub id: String,
    /// The repo ID of the already-indexed repository
    pub repo_id: String,
    /// Specific snapshot ID (omit to use latest)
    pub snapshot_id: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteLibProfileParams {
    /// The lib profile ID to delete
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddStackRuleParams {
    /// Rule type (e.g. "prefer", "avoid", "guideline", "constraint")
    pub rule_type: String,
    /// Subject area (e.g. "HTTP framework", "database", "serialization")
    pub subject: String,
    /// Rule content — the actual recommendation or constraint
    pub content: String,
    /// Optional reference to a lib profile ID
    pub lib_profile_id: Option<String>,
    /// Priority (higher = more important, default: 0)
    pub priority: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListStackRulesParams {
    /// Filter by rule type
    pub rule_type: Option<String>,
    /// Filter by subject
    pub subject: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteStackRuleParams {
    /// The stack rule ID to delete
    pub id: i64,
}

#[derive(Deserialize, JsonSchema)]
pub struct ArchitectAdviseParams {
    /// Natural language question about technology choices or architecture
    pub question: String,
    /// Maximum number of relevant items to consider (default: 5)
    pub limit: Option<i64>,
}

// --- Project Profile params ---

#[derive(Deserialize, JsonSchema)]
pub struct CreateProjectProfileParams {
    /// Unique project ID (e.g. "my-api", "frontend-app")
    pub id: String,
    /// Optional repo ID to link the project to an indexed repository
    pub repo_id: Option<String>,
    /// Human-readable project name
    pub name: String,
    /// Project description
    pub description: Option<String>,
    /// Technology stack as JSON array: [{"lib": "axum", "role": "HTTP framework", "why": "..."}]
    pub stack: Option<serde_json::Value>,
    /// Technical constraints (e.g. "must support WASM", "no unsafe code")
    pub constraints: Option<String>,
    /// Code style preferences (e.g. "builder pattern for config", "anyhow for errors")
    pub code_style: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetProjectProfileParams {
    /// The project profile ID
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListProjectProfilesParams {}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteProjectProfileParams {
    /// The project profile ID to delete
    pub id: String,
}

// --- Decision params ---

#[derive(Deserialize, JsonSchema)]
pub struct RecordDecisionParams {
    /// Title of the decision (e.g. "Use Axum for HTTP layer")
    pub title: String,
    /// Context: what problem this decision addresses
    pub context: Option<String>,
    /// The choice that was made
    pub choice: String,
    /// Alternatives that were considered
    pub alternatives: Option<String>,
    /// Reasoning behind the choice
    pub reasoning: Option<String>,
    /// Optional project profile ID to associate this decision with
    pub project_profile_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListDecisionsParams {
    /// Filter by project profile ID
    pub project_profile_id: Option<String>,
    /// Filter by status: "active", "superseded", or "reverted"
    pub status: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateDecisionParams {
    /// The decision ID to update
    pub id: i64,
    /// Outcome description (what actually happened)
    pub outcome: Option<String>,
    /// New status: "active", "superseded", or "reverted"
    pub status: Option<String>,
}

// --- Compare params ---

#[derive(Deserialize, JsonSchema)]
pub struct CompareLibsParams {
    /// List of library profile IDs to compare (e.g. ["axum", "actix-web"])
    pub lib_ids: Vec<String>,
    /// Criteria for comparison (e.g. "building a REST API with WebSocket support")
    pub criteria: String,
}

// --- Pattern params ---

#[derive(Deserialize, JsonSchema)]
pub struct AddPatternParams {
    /// Pattern name (e.g. "JWT auth with axum + tower")
    pub name: String,
    /// Category (e.g. "authentication", "error-handling", "database")
    pub category: Option<String>,
    /// Brief description of the pattern
    pub description: Option<String>,
    /// Library IDs involved in this pattern
    pub libs_involved: Option<Vec<String>>,
    /// The pattern content: code examples, steps, best practices
    pub pattern_text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListPatternsParams {
    /// Filter by category
    pub category: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetPatternParams {
    /// The pattern ID
    pub id: i64,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeletePatternParams {
    /// The pattern ID to delete
    pub id: i64,
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
