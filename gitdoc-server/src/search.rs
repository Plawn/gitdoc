use anyhow::Result;
use std::path::Path;
use std::sync::Mutex;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

// --- Field handle structs ---

struct DocFields {
    file_id: Field,
    title: Field,
    content: Field,
    file_path: Field,
}

struct SymbolFields {
    file_id: Field,
    symbol_id: Field,
    name: Field,
    qualified_name: Field,
    kind: Field,
    visibility: Field,
    signature: Field,
    doc_comment: Field,
    file_path: Field,
}

// --- Public types ---

pub struct SymbolSearchEntry {
    pub file_id: i64,
    pub symbol_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub file_path: String,
}

#[derive(Debug, serde::Serialize)]
pub struct DocSearchResult {
    pub file_path: String,
    pub title: String,
    pub snippets: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
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

// --- SearchIndex ---

pub struct SearchIndex {
    doc_writer: Mutex<IndexWriter>,
    doc_reader: IndexReader,
    doc_fields: DocFields,
    sym_writer: Mutex<IndexWriter>,
    sym_reader: IndexReader,
    sym_fields: SymbolFields,
}

fn build_doc_schema() -> (Schema, DocFields) {
    let mut builder = Schema::builder();
    let file_id = builder.add_u64_field("file_id", INDEXED | FAST | STORED);
    let title = builder.add_text_field("title", TEXT | STORED);
    let content = builder.add_text_field("content", TEXT | STORED);
    let file_path = builder.add_text_field("file_path", STRING | STORED);
    (builder.build(), DocFields { file_id, title, content, file_path })
}

fn build_symbol_schema() -> (Schema, SymbolFields) {
    let mut builder = Schema::builder();
    let file_id = builder.add_u64_field("file_id", INDEXED | FAST | STORED);
    let symbol_id = builder.add_u64_field("symbol_id", INDEXED | STORED);
    let name = builder.add_text_field("name", TEXT | STORED);
    let qualified_name = builder.add_text_field("qualified_name", STRING | STORED);
    let kind = builder.add_text_field("kind", STRING | STORED);
    let visibility = builder.add_text_field("visibility", STRING | STORED);
    let signature = builder.add_text_field("signature", TEXT | STORED);
    let doc_comment = builder.add_text_field("doc_comment", TEXT | STORED);
    let file_path = builder.add_text_field("file_path", STRING | STORED);
    (
        builder.build(),
        SymbolFields { file_id, symbol_id, name, qualified_name, kind, visibility, signature, doc_comment, file_path },
    )
}

fn build_file_id_filter(file_id_field: Field, file_ids: &[i64]) -> Box<dyn tantivy::query::Query> {
    if file_ids.is_empty() {
        return Box::new(BooleanQuery::new(Vec::new()));
    }
    let clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = file_ids
        .iter()
        .map(|&id| {
            let term = Term::from_field_u64(file_id_field, id as u64);
            (
                Occur::Should,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)) as Box<dyn tantivy::query::Query>,
            )
        })
        .collect();
    Box::new(BooleanQuery::new(clauses))
}

impl SearchIndex {
    /// Open or create disk-based indexes at the given directory.
    pub fn open(index_dir: &Path) -> Result<Self> {
        let doc_dir = index_dir.join("docs");
        let sym_dir = index_dir.join("symbols");
        std::fs::create_dir_all(&doc_dir)?;
        std::fs::create_dir_all(&sym_dir)?;

        let (doc_schema, doc_fields) = build_doc_schema();
        let doc_index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(&doc_dir)?,
            doc_schema,
        )?;
        let doc_writer = doc_index.writer(50_000_000)?;
        let doc_reader = doc_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let (sym_schema, sym_fields) = build_symbol_schema();
        let sym_index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(&sym_dir)?,
            sym_schema,
        )?;
        let sym_writer = sym_index.writer(50_000_000)?;
        let sym_reader = sym_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            doc_writer: Mutex::new(doc_writer),
            doc_reader,
            doc_fields,
            sym_writer: Mutex::new(sym_writer),
            sym_reader,
            sym_fields,
        })
    }

    /// Create in-memory indexes for testing.
    pub fn open_in_memory() -> Result<Self> {
        let (doc_schema, doc_fields) = build_doc_schema();
        let doc_index = Index::create_in_ram(doc_schema);
        let doc_writer = doc_index.writer(15_000_000)?;
        let doc_reader = doc_index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        let (sym_schema, sym_fields) = build_symbol_schema();
        let sym_index = Index::create_in_ram(sym_schema);
        let sym_writer = sym_index.writer(15_000_000)?;
        let sym_reader = sym_index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            doc_writer: Mutex::new(doc_writer),
            doc_reader,
            doc_fields,
            sym_writer: Mutex::new(sym_writer),
            sym_reader,
            sym_fields,
        })
    }

    /// Add a doc to the index (does not commit).
    pub fn add_doc(
        &self,
        file_id: i64,
        file_path: &str,
        title: Option<&str>,
        content: &str,
    ) -> Result<()> {
        let writer = self.doc_writer.lock().unwrap();
        let mut doc = TantivyDocument::new();
        doc.add_u64(self.doc_fields.file_id, file_id as u64);
        doc.add_text(self.doc_fields.file_path, file_path);
        doc.add_text(self.doc_fields.title, title.unwrap_or(""));
        doc.add_text(self.doc_fields.content, content);
        writer.add_document(doc)?;
        Ok(())
    }

    /// Add symbols to the index (does not commit).
    pub fn add_symbols(&self, entries: &[SymbolSearchEntry]) -> Result<()> {
        let writer = self.sym_writer.lock().unwrap();
        for sym in entries {
            let mut doc = TantivyDocument::new();
            doc.add_u64(self.sym_fields.file_id, sym.file_id as u64);
            doc.add_u64(self.sym_fields.symbol_id, sym.symbol_id as u64);
            doc.add_text(self.sym_fields.name, &sym.name);
            doc.add_text(self.sym_fields.qualified_name, &sym.qualified_name);
            doc.add_text(self.sym_fields.kind, &sym.kind);
            doc.add_text(self.sym_fields.visibility, &sym.visibility);
            doc.add_text(self.sym_fields.signature, &sym.signature);
            doc.add_text(
                self.sym_fields.doc_comment,
                sym.doc_comment.as_deref().unwrap_or(""),
            );
            doc.add_text(self.sym_fields.file_path, &sym.file_path);
            writer.add_document(doc)?;
        }
        Ok(())
    }

    /// Commit both writers and reload readers.
    pub fn commit_all(&self) -> Result<()> {
        {
            let mut writer = self.doc_writer.lock().unwrap();
            writer.commit()?;
        }
        {
            let mut writer = self.sym_writer.lock().unwrap();
            writer.commit()?;
        }
        self.doc_reader.reload()?;
        self.sym_reader.reload()?;
        Ok(())
    }

    /// Search docs, filtered to file_ids belonging to the given snapshot.
    pub fn search_docs(
        &self,
        query_str: &str,
        file_ids: &[i64],
        limit: usize,
    ) -> Result<Vec<DocSearchResult>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.doc_reader.searcher();
        let query_parser = QueryParser::for_index(
            searcher.index(),
            vec![self.doc_fields.title, self.doc_fields.content],
        );
        let text_query = query_parser.parse_query(query_str)?;

        let file_id_filter = build_file_id_filter(self.doc_fields.file_id, file_ids);
        let combined = BooleanQuery::new(vec![
            (Occur::Must, text_query),
            (Occur::Must, file_id_filter),
        ]);

        let top_docs = searcher.search(&combined, &TopDocs::with_limit(limit))?;

        let snippet_gen = SnippetGenerator::create(&searcher, &combined, self.doc_fields.content)?;

        let mut results = Vec::new();
        for (_score, doc_addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_addr)?;
            let file_path = doc
                .get_first(self.doc_fields.file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = doc
                .get_first(self.doc_fields.title)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let snippet = snippet_gen.snippet_from_doc(&doc);
            let snippet_text = snippet.to_html();
            let snippets = if snippet_text.is_empty() {
                Vec::new()
            } else {
                vec![snippet_text]
            };

            results.push(DocSearchResult {
                file_path,
                title,
                snippets,
            });
        }

        Ok(results)
    }

    /// Search symbols, filtered to file_ids belonging to the given snapshot.
    pub fn search_symbols(
        &self,
        query_str: &str,
        file_ids: &[i64],
        kind_filter: Option<&str>,
        visibility_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SymbolSearchResult>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.sym_reader.searcher();
        let query_parser = QueryParser::for_index(
            searcher.index(),
            vec![
                self.sym_fields.name,
                self.sym_fields.signature,
                self.sym_fields.doc_comment,
            ],
        );
        let text_query = query_parser.parse_query(query_str)?;

        let file_id_filter = build_file_id_filter(self.sym_fields.file_id, file_ids);
        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = vec![
            (Occur::Must, text_query),
            (Occur::Must, file_id_filter),
        ];

        if let Some(kind) = kind_filter {
            let term = Term::from_field_text(self.sym_fields.kind, kind);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if let Some(vis) = visibility_filter {
            let term = Term::from_field_text(self.sym_fields.visibility, vis);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        let combined = BooleanQuery::new(clauses);
        let top_docs = searcher.search(&combined, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_addr)?;

            let get_text = |field: Field| -> String {
                doc.get_first(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let get_u64 = |field: Field| -> u64 {
                doc.get_first(field)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            };

            let doc_comment_str = get_text(self.sym_fields.doc_comment);
            results.push(SymbolSearchResult {
                symbol_id: get_u64(self.sym_fields.symbol_id) as i64,
                name: get_text(self.sym_fields.name),
                qualified_name: get_text(self.sym_fields.qualified_name),
                kind: get_text(self.sym_fields.kind),
                visibility: get_text(self.sym_fields.visibility),
                signature: get_text(self.sym_fields.signature),
                doc_comment: if doc_comment_str.is_empty() {
                    None
                } else {
                    Some(doc_comment_str)
                },
                file_path: get_text(self.sym_fields.file_path),
                score,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_search_docs() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_doc(1, "README.md", Some("Getting Started"), "This is a guide to getting started with the project.").unwrap();
        idx.add_doc(2, "docs/auth.md", Some("Authentication"), "How to authenticate users with JWT tokens.").unwrap();
        idx.commit_all().unwrap();

        let results = idx.search_docs("authenticate", &[1, 2], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "docs/auth.md");

        // File_id filter restricts results
        let results = idx.search_docs("guide", &[2], 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn index_and_search_symbols() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_symbols(&[
            SymbolSearchEntry {
                file_id: 1, symbol_id: 10,
                name: "authenticate".into(),
                qualified_name: "src/auth.rs::authenticate".into(),
                kind: "function".into(), visibility: "pub".into(),
                signature: "pub fn authenticate(token: &str) -> Result<User>".into(),
                doc_comment: Some("Verify a JWT token and return the user.".into()),
                file_path: "src/auth.rs".into(),
            },
            SymbolSearchEntry {
                file_id: 1, symbol_id: 11,
                name: "User".into(),
                qualified_name: "src/auth.rs::User".into(),
                kind: "struct".into(), visibility: "pub".into(),
                signature: "pub struct User".into(),
                doc_comment: None,
                file_path: "src/auth.rs".into(),
            },
        ]).unwrap();
        idx.commit_all().unwrap();

        let results = idx.search_symbols("authenticate", &[1], None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "authenticate");

        // Kind filter
        let results = idx.search_symbols("User", &[1], Some("struct"), None, 10).unwrap();
        assert_eq!(results.len(), 1);

        let results = idx.search_symbols("authenticate", &[1], Some("struct"), None, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn empty_file_ids_returns_no_results() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_doc(1, "README.md", Some("Title"), "Some content here.").unwrap();
        idx.commit_all().unwrap();
        let results = idx.search_docs("content", &[], 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_by_signature_and_doc_comment() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_symbols(&[
            SymbolSearchEntry {
                file_id: 1, symbol_id: 20,
                name: "process".into(),
                qualified_name: "src/lib.rs::process".into(),
                kind: "function".into(), visibility: "pub".into(),
                signature: "pub fn process(data: Vec<u8>) -> Result<Output>".into(),
                doc_comment: Some("Processes raw binary data into structured output.".into()),
                file_path: "src/lib.rs".into(),
            },
        ]).unwrap();
        idx.commit_all().unwrap();

        // Find by signature content
        let results = idx.search_symbols("binary", &[1], None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "process");

        // Find by doc comment
        let results = idx.search_symbols("structured output", &[1], None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
    }
}
