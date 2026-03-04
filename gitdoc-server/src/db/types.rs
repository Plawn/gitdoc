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
