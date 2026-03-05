use serde;
use sqlx;

// --- GC stats ---

#[derive(Debug, serde::Serialize)]
pub struct GcStats {
    pub files_removed: u64,
    pub docs_removed: u64,
    pub symbols_removed: u64,
    pub refs_removed: u64,
    pub embeddings_removed: u64,
}

// --- Row types ---

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct RepoRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SnapshotRow {
    pub id: i64,
    pub repo_id: String,
    pub commit_sha: String,
    pub label: Option<String>,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub stats: Option<String>,
}

#[derive(Debug)]
pub struct SymbolInsert {
    pub file_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub byte_start: i64,
    pub byte_end: i64,
    pub parent_id: Option<i64>,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub body: String,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct DocRow {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct DocContent {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
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

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
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

#[derive(Debug, Default)]
pub struct SymbolFilters {
    pub kind: Option<String>,
    pub visibility: Option<String>,
    pub file_path: Option<String>,
    pub include_private: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct RefRow {
    pub id: i64,
    pub from_symbol_id: i64,
    pub to_symbol_id: i64,
    pub kind: String,
}

#[derive(Debug, serde::Serialize)]
pub struct RefWithSymbol {
    pub ref_kind: String,
    pub symbol: SymbolRow,
}

/// Internal row type for decoding flat RefWithSymbol query results.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RefWithSymbolRow {
    pub(crate) ref_kind: String,
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) qualified_name: String,
    pub(crate) sym_kind: String,
    pub(crate) visibility: String,
    pub(crate) file_path: String,
    pub(crate) line_start: i64,
    pub(crate) line_end: i64,
    pub(crate) signature: String,
    pub(crate) doc_comment: Option<String>,
    pub(crate) parent_id: Option<i64>,
}

impl From<RefWithSymbolRow> for RefWithSymbol {
    fn from(r: RefWithSymbolRow) -> Self {
        Self {
            ref_kind: r.ref_kind,
            symbol: SymbolRow {
                id: r.id,
                name: r.name,
                qualified_name: r.qualified_name,
                kind: r.sym_kind,
                visibility: r.visibility,
                file_path: r.file_path,
                line_start: r.line_start,
                line_end: r.line_end,
                signature: r.signature,
                doc_comment: r.doc_comment,
                parent_id: r.parent_id,
            },
        }
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SnapshotFileRow {
    pub file_path: String,
    pub file_id: i64,
    pub file_type: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SymbolForRef {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file_path: String,
    pub body: String,
}

#[derive(Debug)]
pub struct EmbeddingInsert {
    pub file_id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub text: String,
    pub vector: Option<pgvector::Vector>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct EmbeddingRow {
    pub id: i64,
    pub file_id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub text: String,
    pub vector: Option<pgvector::Vector>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct EmbeddingSearchResult {
    pub id: i64,
    pub file_id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub text: String,
    pub score: f64,
}

// --- High-level view types ---

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SummaryRow {
    pub id: i64,
    pub snapshot_id: i64,
    pub scope: String,
    pub content: String,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct PublicApiSymbol {
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

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SnapshotFileInfo {
    pub file_path: String,
    pub file_type: String,
    pub public_symbol_count: i64,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ModuleSymbol {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub doc_comment: Option<String>,
    pub parent_id: Option<i64>,
}

// --- Cheatsheet types ---

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct CheatsheetRow {
    pub repo_id: String,
    pub content: String,
    pub snapshot_id: Option<i64>,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct CheatsheetPatchRow {
    pub id: i64,
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
    pub prev_content: String,
    pub new_content: String,
    pub change_summary: String,
    pub trigger: String,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct CheatsheetPatchMeta {
    pub id: i64,
    pub repo_id: String,
    pub snapshot_id: Option<i64>,
    pub change_summary: String,
    pub trigger: String,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Architect types ---

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct LibProfileRow {
    pub id: String,
    pub name: String,
    pub repo_id: Option<String>,
    pub category: String,
    pub version_hint: String,
    pub profile: String,
    pub source: String,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct LibProfileSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version_hint: String,
    pub source: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct StackRuleRow {
    pub id: i64,
    pub rule_type: String,
    pub subject: String,
    pub content: String,
    pub lib_profile_id: Option<String>,
    pub priority: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize)]
pub struct ArchitectSearchResult {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub score: f64,
}

// --- Project Profile types ---

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ProjectProfileRow {
    pub id: String,
    pub repo_id: Option<String>,
    pub name: String,
    pub description: String,
    pub stack: serde_json::Value,
    pub constraints: String,
    pub code_style: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ProjectProfileSummary {
    pub id: String,
    pub name: String,
    pub repo_id: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Architecture Decision types ---

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
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
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Architecture Pattern types ---

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ArchPatternRow {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub description: String,
    pub libs_involved: Vec<String>,
    pub pattern_text: String,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Conversation types ---

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ConversationRow {
    pub id: i64,
    pub snapshot_id: i64,
    pub condensed_context: String,
    pub raw_turn_tokens: i32,
    pub condensed_up_to: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ConversationTurnRow {
    pub id: i64,
    pub conversation_id: i64,
    pub turn_index: i32,
    pub question: String,
    pub answer: String,
    pub sources: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
