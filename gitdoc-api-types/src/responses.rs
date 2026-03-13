use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoRow {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoSummary {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub created_at: String,
    pub snapshot_count: i64,
    pub latest_snapshot_label: Option<String>,
    pub latest_snapshot_commit: Option<String>,
    pub latest_snapshot_indexed_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub repo_id: String,
    pub commit_sha: String,
    pub label: Option<String>,
    pub indexed_at: String,
    pub status: String,
    pub stats: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoDetail {
    pub repo: RepoRow,
    pub snapshots: Vec<SnapshotRow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateRepoResponse {
    pub id: String,
    pub already_existed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FetchRepoResponse {
    pub fetched: bool,
    pub repo_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IndexResult {
    pub snapshot_id: i64,
    pub files_scanned: i64,
    pub docs_count: i64,
    pub symbols_count: i64,
    pub refs_count: usize,
    pub embeddings_count: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Docs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct DocRow {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocContent {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub content: Option<String>,
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolRow {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolDetail {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub body: String,
    pub parent_id: Option<i64>,
    pub children_count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolDetailResponse {
    pub symbol: SymbolDetail,
    pub children: Vec<SymbolRow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SnapshotSymbolResponse {
    pub symbol: SymbolDetail,
    pub children: Vec<SymbolRow>,
    pub referenced_by_count: i64,
    pub references_count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchSymbolsResponse {
    pub symbols: Vec<SymbolDetail>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolContextResponse {
    pub symbol: SymbolDetail,
    pub children: Vec<SymbolRow>,
    pub referenced_by_count: i64,
    pub references_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<RefWithSymbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<RefWithSymbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations: Option<Vec<RefWithSymbol>>,
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct OverviewSymbol {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub signature: String,
    pub doc_comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OverviewResponse {
    pub snapshot: SnapshotRow,
    pub readme: Option<String>,
    pub docs: Vec<DocRow>,
    pub top_level_symbols: Vec<OverviewSymbol>,
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct RefWithSymbol {
    pub ref_kind: String,
    pub symbol: SymbolRow,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct DocSearchResult {
    pub file_path: String,
    pub title: String,
    pub snippets: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolSearchResult {
    pub symbol_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub file_path: String,
    pub score: f32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticSearchResult {
    pub source_type: String,
    pub source_id: i64,
    pub score: f64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<SemanticDocHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SemanticSymbolHit>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticDocHit {
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticSymbolHit {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    pub file_path: String,
    pub line_start: i64,
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSymbolEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSigVis {
    pub signature: String,
    pub visibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModifiedSymbol {
    pub qualified_name: String,
    pub kind: String,
    pub changes: Vec<String>,
    pub from: DiffSigVis,
    pub to: DiffSigVis,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSummary {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffResponse {
    pub from_snapshot: i64,
    pub to_snapshot: i64,
    pub added: Vec<DiffSymbolEntry>,
    pub removed: Vec<DiffSymbolEntry>,
    pub modified: Vec<ModifiedSymbol>,
    pub summary: DiffSummary,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSummarizeResponse {
    pub from_snapshot: i64,
    pub to_snapshot: i64,
    pub changelog: String,
    pub stats: DiffSummary,
}

// ---------------------------------------------------------------------------
// Public API / Module tree
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct PublicApiMethod {
    pub id: i64,
    pub name: String,
    pub signature: String,
    pub doc_comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PublicApiEntry {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub methods: Vec<PublicApiMethod>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PublicApiResponse {
    pub snapshot_id: i64,
    pub module_path: Option<String>,
    pub modules: BTreeMap<String, Vec<PublicApiEntry>>,
    pub total_items: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleTreeSymbol {
    pub name: String,
    pub kind: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModuleTreeNode {
    pub name: String,
    pub path: String,
    pub doc_comment: Option<String>,
    pub public_items: i64,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<ModuleTreeNode>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub symbols: Vec<ModuleTreeSymbol>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModuleTreeResponse {
    pub snapshot_id: i64,
    pub tree: Vec<ModuleTreeNode>,
}

// ---------------------------------------------------------------------------
// Type context / Examples
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct UsedBy {
    pub callers: Vec<RefWithSymbol>,
    pub type_users: Vec<RefWithSymbol>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DependsOn {
    pub types: Vec<RefWithSymbol>,
    pub calls: Vec<RefWithSymbol>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeContextResponse {
    pub symbol: SymbolDetail,
    pub methods: Vec<SymbolRow>,
    pub fields: Vec<SymbolRow>,
    pub traits_implemented: Vec<RefWithSymbol>,
    pub implementors: Vec<RefWithSymbol>,
    pub used_by: UsedBy,
    pub depends_on: DependsOn,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CodeExample {
    pub language: Option<String>,
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExamplesResponse {
    pub symbol_id: i64,
    pub symbol_name: String,
    pub examples: Vec<CodeExample>,
}

// ---------------------------------------------------------------------------
// Explain
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct MethodInfo {
    pub name: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelevantSymbol {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub file_path: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub methods: Vec<MethodInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub traits: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelevantDoc {
    pub file_path: String,
    pub title: Option<String>,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExplainResult {
    pub query: String,
    pub relevant_symbols: Vec<RelevantSymbol>,
    pub relevant_docs: Vec<RelevantDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthesis: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct ConversationResponse {
    pub conversation_id: i64,
    pub answer: String,
    pub sources: Vec<SourceRef>,
    pub turn_index: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SourceRef {
    pub kind: String,
    pub name: String,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(bound = "T: Serialize + for<'a> Deserialize<'a>")]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// ---------------------------------------------------------------------------
// Summarize
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct SummarizeResponse {
    pub snapshot_id: i64,
    pub scope: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SummaryRow {
    pub id: i64,
    pub snapshot_id: i64,
    pub scope: String,
    pub content: String,
    pub model: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Cheatsheet
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct CheatsheetResponse {
    pub repo_id: String,
    pub content: String,
    pub model: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GenerateCheatsheetResponse {
    pub repo_id: String,
    pub patch_id: i64,
    pub content: String,
    pub model: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CheatsheetPatchMeta {
    pub id: i64,
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
    pub change_summary: String,
    pub trigger: String,
    pub model: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CheatsheetPatchRow {
    pub id: i64,
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
    pub prev_content: String,
    pub new_content: String,
    pub change_summary: String,
    pub trigger: String,
    pub model: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Architect — Libs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct LibProfileSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version_hint: String,
    pub source: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LibProfileRow {
    pub id: String,
    pub name: String,
    pub repo_id: Option<String>,
    pub category: String,
    pub version_hint: String,
    pub profile: String,
    pub source: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Architect — Rules
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct StackRuleRow {
    pub id: i64,
    pub rule_type: String,
    pub subject: String,
    pub content: String,
    pub lib_profile_id: Option<String>,
    pub priority: i32,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Architect — Advise / Compare
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct ArchitectSearchResult {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub score: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdviseResponse {
    pub answer: String,
    pub relevant_libs: Vec<ArchitectSearchResult>,
    pub relevant_rules: Vec<ArchitectSearchResult>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompareLibsResponse {
    pub comparison: String,
}

// ---------------------------------------------------------------------------
// Architect — Projects
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectProfileRow {
    pub id: String,
    pub repo_id: Option<String>,
    pub name: String,
    pub description: String,
    pub stack: serde_json::Value,
    pub constraints: String,
    pub code_style: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectProfileSummary {
    pub id: String,
    pub name: String,
    pub repo_id: Option<String>,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Architect — Decisions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct ArchDecisionRow {
    pub id: i64,
    pub project_profile_id: Option<String>,
    pub title: String,
    pub context: String,
    pub choice: String,
    pub alternatives: String,
    pub reasoning: String,
    pub outcome: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Architect — Patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct ArchPatternRow {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub description: String,
    pub libs_involved: Vec<String>,
    pub pattern_text: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}
