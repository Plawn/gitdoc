use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateRepoBody {
    pub id: String,
    pub name: String,
    pub url: String,
}

fn default_commit() -> String {
    "HEAD".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IndexBody {
    #[serde(default = "default_commit")]
    pub commit: String,
    pub label: Option<String>,
    #[serde(default)]
    pub fetch: bool,
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SymbolQuery {
    pub kind: Option<String>,
    pub visibility: Option<String>,
    pub file_path: Option<String>,
    pub include_private: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RefQuery {
    pub direction: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocSearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SymbolSearchQuery {
    pub q: String,
    pub kind: Option<String>,
    pub visibility: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SemanticSearchQuery {
    pub q: String,
    pub scope: Option<String>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Snapshots / Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiffQuery {
    pub kind: Option<String>,
    pub include_private: Option<bool>,
    pub include_source: Option<bool>,
}

// ---------------------------------------------------------------------------
// Batch symbols
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BatchSymbolsRequest {
    pub ids: Vec<i64>,
}

// ---------------------------------------------------------------------------
// Symbol context
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SymbolContextQuery {
    pub include: Option<String>,
}

// ---------------------------------------------------------------------------
// Summaries
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SummarizeQuery {
    pub scope: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SummaryQuery {
    pub scope: Option<String>,
}

// ---------------------------------------------------------------------------
// Explain
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExplainQuery {
    pub q: String,
    pub synthesize: Option<bool>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Converse
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ConverseRequest {
    pub q: String,
    pub conversation_id: Option<i64>,
    pub limit: Option<usize>,
    pub detail_level: Option<String>,
}

// ---------------------------------------------------------------------------
// Module tree
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ModuleTreeQuery {
    pub depth: Option<usize>,
    pub include_signatures: Option<bool>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PublicApiQuery {
    pub module_path: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Cheatsheet
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GenerateCheatsheetRequest {
    pub snapshot_id: i64,
    pub trigger: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PatchListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Pagination (shared)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Architect — Libs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListLibsQuery {
    pub category: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateLibRequest {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub version_hint: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GenerateLibProfileRequest {
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
}

// ---------------------------------------------------------------------------
// Architect — Rules
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListRulesQuery {
    pub rule_type: Option<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct UpsertRuleRequest {
    pub id: Option<i64>,
    pub rule_type: String,
    pub subject: String,
    pub content: String,
    pub lib_profile_id: Option<String>,
    pub priority: Option<i32>,
}

// ---------------------------------------------------------------------------
// Architect — Advise
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AdviseRequest {
    pub question: String,
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------------------
// Architect — Compare
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompareLibsRequest {
    pub lib_ids: Vec<String>,
    pub criteria: String,
}

// ---------------------------------------------------------------------------
// Architect — Projects
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateProjectProfileRequest {
    pub id: String,
    pub repo_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub stack: Option<serde_json::Value>,
    pub constraints: Option<String>,
    pub code_style: Option<String>,
}

// ---------------------------------------------------------------------------
// Architect — Decisions
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateDecisionRequest {
    pub project_profile_id: Option<String>,
    pub title: String,
    pub context: Option<String>,
    pub choice: String,
    pub alternatives: Option<String>,
    pub reasoning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListDecisionsQuery {
    pub project_profile_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct UpdateDecisionRequest {
    pub outcome: Option<String>,
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Architect — Patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreatePatternRequest {
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub libs_involved: Option<Vec<String>>,
    pub pattern_text: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListPatternsQuery {
    pub category: Option<String>,
}
