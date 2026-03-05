use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoRow {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub created_at: String,
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
pub struct OverviewResponse {
    pub snapshot: SnapshotRow,
    pub readme: Option<String>,
    pub docs: Vec<DocRow>,
    pub top_level_symbols: Vec<SymbolRow>,
}

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
pub struct RefWithSymbol {
    pub ref_kind: String,
    pub symbol: SymbolRow,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticSearchResult {
    pub source_type: String,
    pub source_id: i64,
    pub score: f32,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSymbolEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModifiedFields {
    pub signature: String,
    pub visibility: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModifiedSymbol {
    pub qualified_name: String,
    pub kind: String,
    pub changes: Vec<String>,
    pub from: ModifiedFields,
    pub to: ModifiedFields,
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
pub struct PublicApiResponse {
    pub snapshot_id: i64,
    pub module_path: Option<String>,
    pub modules: serde_json::Value,
    pub total_items: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModuleTreeResponse {
    pub snapshot_id: i64,
    pub tree: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeContextResponse {
    pub symbol: SymbolDetail,
    pub methods: Vec<SymbolRow>,
    pub fields: Vec<SymbolRow>,
    pub traits_implemented: Vec<RefWithSymbol>,
    pub implementors: Vec<RefWithSymbol>,
    pub used_by: serde_json::Value,
    pub depends_on: serde_json::Value,
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
pub struct SummarizeResponse {
    pub snapshot_id: i64,
    pub scope: String,
    pub content: String,
}
