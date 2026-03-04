use anyhow::Result;
use rusqlite::{Connection, params, params_from_iter};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS repos (
                id          TEXT PRIMARY KEY,
                path        TEXT NOT NULL,
                name        TEXT NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE IF NOT EXISTS snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                repo_id     TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
                commit_sha  TEXT NOT NULL,
                label       TEXT,
                indexed_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                status      TEXT NOT NULL DEFAULT 'indexing',
                stats       TEXT,
                UNIQUE(repo_id, commit_sha)
            );

            CREATE TABLE IF NOT EXISTS files (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                checksum    TEXT NOT NULL UNIQUE,
                content     TEXT
            );

            CREATE TABLE IF NOT EXISTS snapshot_files (
                snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                file_path   TEXT NOT NULL,
                file_id     INTEGER NOT NULL REFERENCES files(id),
                file_type   TEXT NOT NULL DEFAULT 'other',
                PRIMARY KEY (snapshot_id, file_path)
            );

            CREATE TABLE IF NOT EXISTS docs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id     INTEGER NOT NULL REFERENCES files(id),
                title       TEXT
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id         INTEGER NOT NULL REFERENCES files(id),
                name            TEXT NOT NULL,
                qualified_name  TEXT NOT NULL,
                kind            TEXT NOT NULL,
                visibility      TEXT NOT NULL DEFAULT 'private',
                file_path       TEXT NOT NULL,
                line_start      INTEGER NOT NULL,
                line_end        INTEGER NOT NULL,
                byte_start      INTEGER NOT NULL,
                byte_end        INTEGER NOT NULL,
                parent_id       INTEGER REFERENCES symbols(id),
                signature       TEXT NOT NULL,
                doc_comment     TEXT,
                body            TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);

            CREATE TABLE IF NOT EXISTS refs (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                from_symbol_id  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
                to_symbol_id    INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
                kind            TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_refs_from ON refs(from_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_refs_to ON refs(to_symbol_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_refs_unique ON refs(from_symbol_id, to_symbol_id, kind);

            CREATE TABLE IF NOT EXISTS embeddings (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                source_type TEXT NOT NULL,
                source_id   INTEGER NOT NULL,
                text        TEXT NOT NULL,
                vector      BLOB
            );
            ",
        )?;
        Ok(())
    }

    // --- Repos ---

    pub fn insert_repo(&self, id: &str, path: &str, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO repos (id, path, name) VALUES (?1, ?2, ?3)",
            params![id, path, name],
        )?;
        Ok(())
    }

    pub fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, name, created_at FROM repos")?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoRow {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_repo(&self, id: &str) -> Result<Option<RepoRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, name, created_at FROM repos WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(RepoRow {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    // --- Snapshots ---

    pub fn find_snapshot(&self, repo_id: &str, commit_sha: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM snapshots WHERE repo_id = ?1 AND commit_sha = ?2",
        )?;
        let mut rows = stmt.query_map(params![repo_id, commit_sha], |row| row.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn create_snapshot(&self, repo_id: &str, commit_sha: &str, label: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO snapshots (repo_id, commit_sha, label, status) VALUES (?1, ?2, ?3, 'indexing')",
            params![repo_id, commit_sha, label],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finalize_snapshot(&self, snapshot_id: i64, stats_json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshots SET status = 'ready', stats = ?1 WHERE id = ?2",
            params![stats_json, snapshot_id],
        )?;
        Ok(())
    }

    pub fn fail_snapshot(&self, snapshot_id: i64, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshots SET status = 'failed', stats = ?1 WHERE id = ?2",
            params![error, snapshot_id],
        )?;
        Ok(())
    }

    pub fn list_snapshots(&self, repo_id: &str) -> Result<Vec<SnapshotRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repo_id, commit_sha, label, indexed_at, status, stats FROM snapshots WHERE repo_id = ?1 ORDER BY indexed_at DESC",
        )?;
        let rows = stmt.query_map(params![repo_id], |row| {
            Ok(SnapshotRow {
                id: row.get(0)?,
                repo_id: row.get(1)?,
                commit_sha: row.get(2)?,
                label: row.get(3)?,
                indexed_at: row.get(4)?,
                status: row.get(5)?,
                stats: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // --- Files ---

    pub fn find_file_by_checksum(&self, checksum: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM files WHERE checksum = ?1")?;
        let mut rows = stmt.query_map(params![checksum], |row| row.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn insert_file(&self, checksum: &str, content: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO files (checksum, content) VALUES (?1, ?2)",
            params![checksum, content],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_snapshot_file(
        &self,
        snapshot_id: i64,
        file_path: &str,
        file_id: i64,
        file_type: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO snapshot_files (snapshot_id, file_path, file_id, file_type) VALUES (?1, ?2, ?3, ?4)",
            params![snapshot_id, file_path, file_id, file_type],
        )?;
        Ok(())
    }

    // --- Docs ---

    pub fn insert_doc(&self, file_id: i64, title: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO docs (file_id, title) VALUES (?1, ?2)",
            params![file_id, title],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn doc_exists_for_file(&self, file_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM docs WHERE file_id = ?1",
            params![file_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // --- Symbols ---

    pub fn insert_symbol(&self, s: &SymbolInsert) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO symbols (file_id, name, qualified_name, kind, visibility, file_path, line_start, line_end, byte_start, byte_end, parent_id, signature, doc_comment, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                s.file_id, s.name, s.qualified_name, s.kind, s.visibility,
                s.file_path, s.line_start, s.line_end, s.byte_start, s.byte_end,
                s.parent_id, s.signature, s.doc_comment, s.body,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn symbols_exist_for_file(&self, file_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_id = ?1",
            params![file_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn count_docs_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM docs d
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = ?1",
            params![snapshot_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_symbols_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ?1",
            params![snapshot_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_files_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM snapshot_files WHERE snapshot_id = ?1",
            params![snapshot_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // --- Navigation queries ---

    pub fn get_snapshot(&self, id: i64) -> Result<Option<SnapshotRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repo_id, commit_sha, label, indexed_at, status, stats FROM snapshots WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SnapshotRow {
                id: row.get(0)?,
                repo_id: row.get(1)?,
                commit_sha: row.get(2)?,
                label: row.get(3)?,
                indexed_at: row.get(4)?,
                status: row.get(5)?,
                stats: row.get(6)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_docs_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<DocRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT d.id, sf.file_path, d.title
             FROM docs d
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = ?1
             ORDER BY sf.file_path",
        )?;
        let rows = stmt.query_map(params![snapshot_id], |row| {
            Ok(DocRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_doc_content(&self, snapshot_id: i64, file_path: &str) -> Result<Option<DocContent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT d.id, sf.file_path, d.title, f.content
             FROM docs d
             JOIN files f ON f.id = d.file_id
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = ?1 AND sf.file_path = ?2",
        )?;
        let mut rows = stmt.query_map(params![snapshot_id, file_path], |row| {
            Ok(DocContent {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_symbols_for_snapshot(&self, snapshot_id: i64, filters: &SymbolFilters) -> Result<Vec<SymbolRow>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ?",
        );
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(snapshot_id)];

        if let Some(ref kind) = filters.kind {
            sql.push_str(" AND s.kind = ?");
            values.push(Box::new(kind.clone()));
        }
        if let Some(ref visibility) = filters.visibility {
            sql.push_str(" AND s.visibility = ?");
            values.push(Box::new(visibility.clone()));
        }
        if let Some(ref fp) = filters.file_path {
            sql.push_str(" AND s.file_path = ?");
            values.push(Box::new(fp.clone()));
        }
        if !filters.include_private {
            sql.push_str(" AND s.visibility != 'private'");
        }

        sql.push_str(" ORDER BY s.file_path, s.line_start");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter().map(|v| v.as_ref())), |row| {
            Ok(SymbolRow {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                visibility: row.get(4)?,
                file_path: row.get(5)?,
                line_start: row.get(6)?,
                line_end: row.get(7)?,
                signature: row.get(8)?,
                doc_comment: row.get(9)?,
                parent_id: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_symbol_by_id(&self, id: i64) -> Result<Option<SymbolDetail>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.body, s.parent_id,
                    (SELECT COUNT(*) FROM symbols c WHERE c.parent_id = s.id) AS children_count
             FROM symbols s
             WHERE s.id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SymbolDetail {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                visibility: row.get(4)?,
                file_path: row.get(5)?,
                line_start: row.get(6)?,
                line_end: row.get(7)?,
                signature: row.get(8)?,
                doc_comment: row.get(9)?,
                body: row.get(10)?,
                parent_id: row.get(11)?,
                children_count: row.get(12)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_symbol_children(&self, parent_id: i64) -> Result<Vec<SymbolRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, qualified_name, kind, visibility, file_path,
                    line_start, line_end, signature, doc_comment, parent_id
             FROM symbols
             WHERE parent_id = ?1
             ORDER BY line_start",
        )?;
        let rows = stmt.query_map(params![parent_id], |row| {
            Ok(SymbolRow {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                visibility: row.get(4)?,
                file_path: row.get(5)?,
                line_start: row.get(6)?,
                line_end: row.get(7)?,
                signature: row.get(8)?,
                doc_comment: row.get(9)?,
                parent_id: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // --- Refs ---

    pub fn insert_refs_batch(&self, refs: &[(i64, i64, &str)]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut count = 0;
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO refs (from_symbol_id, to_symbol_id, kind) VALUES (?1, ?2, ?3)",
            )?;
            for (from_id, to_id, kind) in refs {
                count += stmt.execute(params![from_id, to_id, kind])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn refs_exist_for_file(&self, file_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             WHERE s.file_id = ?1",
            params![file_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_outbound_refs(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
        kind_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT r.kind, s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.from_symbol_id = ?2",
        );
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(snapshot_id), Box::new(symbol_id)];

        if let Some(kind) = kind_filter {
            sql.push_str(" AND r.kind = ?");
            values.push(Box::new(kind.to_string()));
        }
        sql.push_str(" ORDER BY s.name LIMIT ?");
        values.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter().map(|v| v.as_ref())), |row| {
            Ok(RefWithSymbol {
                ref_kind: row.get(0)?,
                symbol: SymbolRow {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    qualified_name: row.get(3)?,
                    kind: row.get(4)?,
                    visibility: row.get(5)?,
                    file_path: row.get(6)?,
                    line_start: row.get(7)?,
                    line_end: row.get(8)?,
                    signature: row.get(9)?,
                    doc_comment: row.get(10)?,
                    parent_id: row.get(11)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_inbound_refs(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
        kind_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT r.kind, s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.to_symbol_id = ?2",
        );
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(snapshot_id), Box::new(symbol_id)];

        if let Some(kind) = kind_filter {
            sql.push_str(" AND r.kind = ?");
            values.push(Box::new(kind.to_string()));
        }
        sql.push_str(" ORDER BY s.name LIMIT ?");
        values.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter().map(|v| v.as_ref())), |row| {
            Ok(RefWithSymbol {
                ref_kind: row.get(0)?,
                symbol: SymbolRow {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    qualified_name: row.get(3)?,
                    kind: row.get(4)?,
                    visibility: row.get(5)?,
                    file_path: row.get(6)?,
                    line_start: row.get(7)?,
                    line_end: row.get(8)?,
                    signature: row.get(9)?,
                    doc_comment: row.get(10)?,
                    parent_id: row.get(11)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_implementations(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let conn = self.conn.lock().unwrap();
        let sql =
            "SELECT r.kind, s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.to_symbol_id = ?2 AND r.kind = 'implements'
             UNION
             SELECT r.kind, s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.from_symbol_id = ?2 AND r.kind = 'implements'
             ORDER BY 4"; // order by name (column 4)

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![snapshot_id, symbol_id], |row| {
            Ok(RefWithSymbol {
                ref_kind: row.get(0)?,
                symbol: SymbolRow {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    qualified_name: row.get(3)?,
                    kind: row.get(4)?,
                    visibility: row.get(5)?,
                    file_path: row.get(6)?,
                    line_start: row.get(7)?,
                    line_end: row.get(8)?,
                    signature: row.get(9)?,
                    doc_comment: row.get(10)?,
                    parent_id: row.get(11)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn count_refs_for_symbol(&self, symbol_id: i64, snapshot_id: i64) -> Result<(i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let inbound: i64 = conn.query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.to_symbol_id = ?2",
            params![snapshot_id, symbol_id],
            |row| row.get(0),
        )?;
        let outbound: i64 = conn.query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ?1
             WHERE r.from_symbol_id = ?2",
            params![snapshot_id, symbol_id],
            |row| row.get(0),
        )?;
        Ok((inbound, outbound))
    }

    pub fn get_symbols_with_bodies_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<SymbolForRef>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.file_path, s.body
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ?1
             ORDER BY s.file_id, s.line_start",
        )?;
        let rows = stmt.query_map(params![snapshot_id], |row| {
            Ok(SymbolForRef {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                qualified_name: row.get(3)?,
                kind: row.get(4)?,
                file_path: row.get(5)?,
                body: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_snapshot_files(&self, snapshot_id: i64) -> Result<Vec<SnapshotFileRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT file_path, file_id, file_type FROM snapshot_files WHERE snapshot_id = ?1",
        )?;
        let rows = stmt.query_map(params![snapshot_id], |row| {
            Ok(SnapshotFileRow {
                file_path: row.get(0)?,
                file_id: row.get(1)?,
                file_type: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Open an in-memory database for testing.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }
}

// --- Row types ---

#[derive(Debug, serde::Serialize)]
pub struct RepoRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub repo_id: String,
    pub commit_sha: String,
    pub label: Option<String>,
    pub indexed_at: i64,
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

#[derive(Debug, serde::Serialize)]
pub struct DocRow {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct DocContent {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, serde::Serialize)]
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

#[derive(Debug, serde::Serialize)]
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

#[derive(Debug, serde::Serialize)]
pub struct SnapshotFileRow {
    pub file_path: String,
    pub file_id: i64,
    pub file_type: String,
}

#[derive(Debug)]
pub struct SymbolForRef {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file_path: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn migration_creates_tables() {
        let db = test_db();
        // Verify we can query every table without error
        let conn = db.conn.lock().unwrap();
        for table in &["repos", "snapshots", "files", "snapshot_files", "docs", "symbols", "refs", "embeddings"] {
            conn.execute_batch(&format!("SELECT COUNT(*) FROM {table}")).unwrap();
        }
    }

    #[test]
    fn repo_crud() {
        let db = test_db();
        db.insert_repo("test", "/tmp/test", "Test Repo").unwrap();

        let repos = db.list_repos().unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].id, "test");
        assert_eq!(repos[0].path, "/tmp/test");

        let repo = db.get_repo("test").unwrap().unwrap();
        assert_eq!(repo.name, "Test Repo");

        assert!(db.get_repo("nonexistent").unwrap().is_none());
    }

    #[test]
    fn duplicate_repo_fails() {
        let db = test_db();
        db.insert_repo("dup", "/a", "A").unwrap();
        assert!(db.insert_repo("dup", "/b", "B").is_err());
    }

    #[test]
    fn snapshot_lifecycle() {
        let db = test_db();
        db.insert_repo("r", "/tmp/r", "R").unwrap();

        let sid = db.create_snapshot("r", "abc123", Some("v1")).unwrap();
        assert!(sid > 0);

        let found = db.find_snapshot("r", "abc123").unwrap();
        assert_eq!(found, Some(sid));

        assert!(db.find_snapshot("r", "other").unwrap().is_none());

        // Finalize
        db.finalize_snapshot(sid, r#"{"files":1}"#).unwrap();
        let snaps = db.list_snapshots("r").unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "ready");
        assert_eq!(snaps[0].label.as_deref(), Some("v1"));
    }

    #[test]
    fn snapshot_unique_constraint() {
        let db = test_db();
        db.insert_repo("r", "/tmp/r", "R").unwrap();
        db.create_snapshot("r", "sha1", None).unwrap();
        assert!(db.create_snapshot("r", "sha1", None).is_err());
    }

    #[test]
    fn fail_snapshot() {
        let db = test_db();
        db.insert_repo("r", "/tmp/r", "R").unwrap();
        let sid = db.create_snapshot("r", "sha1", None).unwrap();
        db.fail_snapshot(sid, "something broke").unwrap();
        let snaps = db.list_snapshots("r").unwrap();
        assert_eq!(snaps[0].status, "failed");
    }

    #[test]
    fn file_deduplication() {
        let db = test_db();
        let id1 = db.insert_file("checksum_aaa", Some("hello")).unwrap();
        let id2 = db.find_file_by_checksum("checksum_aaa").unwrap();
        assert_eq!(id2, Some(id1));

        assert!(db.find_file_by_checksum("nonexistent").unwrap().is_none());

        // Duplicate checksum fails
        assert!(db.insert_file("checksum_aaa", None).is_err());
    }

    #[test]
    fn snapshot_files_linking() {
        let db = test_db();
        db.insert_repo("r", "/tmp/r", "R").unwrap();
        let sid = db.create_snapshot("r", "sha1", None).unwrap();
        let fid = db.insert_file("ck1", None).unwrap();
        db.insert_snapshot_file(sid, "src/main.rs", fid, "rust").unwrap();

        assert_eq!(db.count_files_for_snapshot(sid).unwrap(), 1);
    }

    #[test]
    fn docs_for_file() {
        let db = test_db();
        let fid = db.insert_file("ck_md", Some("# Title\ncontent")).unwrap();
        assert!(!db.doc_exists_for_file(fid).unwrap());

        db.insert_doc(fid, Some("Title")).unwrap();
        assert!(db.doc_exists_for_file(fid).unwrap());
    }

    #[test]
    fn symbols_for_file() {
        let db = test_db();
        let fid = db.insert_file("ck_rs", None).unwrap();
        assert!(!db.symbols_exist_for_file(fid).unwrap());

        db.insert_symbol(&SymbolInsert {
            file_id: fid,
            name: "main".into(),
            qualified_name: "src/main.rs::main".into(),
            kind: "function".into(),
            visibility: "pub".into(),
            file_path: "src/main.rs".into(),
            line_start: 1, line_end: 3,
            byte_start: 0, byte_end: 50,
            parent_id: None,
            signature: "pub fn main()".into(),
            doc_comment: None,
            body: "pub fn main() { }".into(),
        }).unwrap();

        assert!(db.symbols_exist_for_file(fid).unwrap());
    }

    #[test]
    fn count_docs_and_symbols_for_snapshot() {
        let db = test_db();
        db.insert_repo("r", "/tmp/r", "R").unwrap();
        let sid = db.create_snapshot("r", "sha1", None).unwrap();

        let fid_md = db.insert_file("ck_md2", Some("# Doc")).unwrap();
        db.insert_snapshot_file(sid, "README.md", fid_md, "doc").unwrap();
        db.insert_doc(fid_md, Some("Doc")).unwrap();

        let fid_rs = db.insert_file("ck_rs2", None).unwrap();
        db.insert_snapshot_file(sid, "lib.rs", fid_rs, "rust").unwrap();
        db.insert_symbol(&SymbolInsert {
            file_id: fid_rs,
            name: "foo".into(),
            qualified_name: "lib.rs::foo".into(),
            kind: "function".into(),
            visibility: "pub".into(),
            file_path: "lib.rs".into(),
            line_start: 1, line_end: 1,
            byte_start: 0, byte_end: 20,
            parent_id: None,
            signature: "pub fn foo()".into(),
            doc_comment: None,
            body: "pub fn foo() {}".into(),
        }).unwrap();

        assert_eq!(db.count_docs_for_snapshot(sid).unwrap(), 1);
        assert_eq!(db.count_symbols_for_snapshot(sid).unwrap(), 1);
        assert_eq!(db.count_files_for_snapshot(sid).unwrap(), 2);
    }
}
