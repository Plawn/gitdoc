use anyhow::Result;
use sqlx::postgres::PgPool;
use sqlx::QueryBuilder;

pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    // --- Repos ---

    pub async fn insert_repo(&self, id: &str, path: &str, name: &str, url: Option<&str>) -> Result<()> {
        sqlx::query("INSERT INTO repos (id, path, name, url) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(path)
            .bind(name)
            .bind(url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let rows = sqlx::query_as::<_, RepoRow>("SELECT id, path, name, url, created_at FROM repos")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_repo(&self, id: &str) -> Result<Option<RepoRow>> {
        let row = sqlx::query_as::<_, RepoRow>(
            "SELECT id, path, name, url, created_at FROM repos WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // --- Snapshots ---

    pub async fn find_snapshot(&self, repo_id: &str, commit_sha: &str) -> Result<Option<i64>> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM snapshots WHERE repo_id = $1 AND commit_sha = $2",
        )
        .bind(repo_id)
        .bind(commit_sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn create_snapshot(
        &self,
        repo_id: &str,
        commit_sha: &str,
        label: Option<&str>,
    ) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO snapshots (repo_id, commit_sha, label, status) VALUES ($1, $2, $3, 'indexing') RETURNING id",
        )
        .bind(repo_id)
        .bind(commit_sha)
        .bind(label)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finalize_snapshot(&self, snapshot_id: i64, stats_json: &str) -> Result<()> {
        sqlx::query("UPDATE snapshots SET status = 'ready', stats = $1 WHERE id = $2")
            .bind(stats_json)
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fail_snapshot(&self, snapshot_id: i64, error: &str) -> Result<()> {
        sqlx::query("UPDATE snapshots SET status = 'failed', stats = $1 WHERE id = $2")
            .bind(error)
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_snapshots(&self, repo_id: &str) -> Result<Vec<SnapshotRow>> {
        let rows = sqlx::query_as::<_, SnapshotRow>(
            "SELECT id, repo_id, commit_sha, label, indexed_at, status, stats FROM snapshots WHERE repo_id = $1 ORDER BY indexed_at DESC",
        )
        .bind(repo_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_snapshot(&self, id: i64) -> Result<Option<SnapshotRow>> {
        let row = sqlx::query_as::<_, SnapshotRow>(
            "SELECT id, repo_id, commit_sha, label, indexed_at, status, stats FROM snapshots WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // --- Files ---

    pub async fn find_file_by_checksum(&self, checksum: &str) -> Result<Option<i64>> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM files WHERE checksum = $1")
                .bind(checksum)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn insert_file(&self, checksum: &str, content: Option<&str>) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO files (checksum, content) VALUES ($1, $2) RETURNING id",
        )
        .bind(checksum)
        .bind(content)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn insert_snapshot_file(
        &self,
        snapshot_id: i64,
        file_path: &str,
        file_id: i64,
        file_type: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO snapshot_files (snapshot_id, file_path, file_id, file_type) VALUES ($1, $2, $3, $4)",
        )
        .bind(snapshot_id)
        .bind(file_path)
        .bind(file_id)
        .bind(file_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- Docs ---

    pub async fn insert_doc(&self, file_id: i64, title: Option<&str>) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO docs (file_id, title) VALUES ($1, $2) RETURNING id",
        )
        .bind(file_id)
        .bind(title)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn doc_exists_for_file(&self, file_id: i64) -> Result<bool> {
        let (exists,): (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM docs WHERE file_id = $1)")
                .bind(file_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    // --- Symbols ---

    pub async fn insert_symbol(&self, s: &SymbolInsert) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO symbols (file_id, name, qualified_name, kind, visibility, file_path, line_start, line_end, byte_start, byte_end, parent_id, signature, doc_comment, body)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING id",
        )
        .bind(s.file_id)
        .bind(&s.name)
        .bind(&s.qualified_name)
        .bind(&s.kind)
        .bind(&s.visibility)
        .bind(&s.file_path)
        .bind(s.line_start)
        .bind(s.line_end)
        .bind(s.byte_start)
        .bind(s.byte_end)
        .bind(s.parent_id)
        .bind(&s.signature)
        .bind(&s.doc_comment)
        .bind(&s.body)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_symbol_parent(&self, symbol_id: i64, parent_id: i64) -> Result<()> {
        sqlx::query("UPDATE symbols SET parent_id = $1 WHERE id = $2")
            .bind(parent_id)
            .bind(symbol_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn symbols_exist_for_file(&self, file_id: i64) -> Result<bool> {
        let (exists,): (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM symbols WHERE file_id = $1)")
                .bind(file_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    pub async fn count_docs_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM docs d
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn count_symbols_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn count_files_for_snapshot(&self, snapshot_id: i64) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM snapshot_files WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    // --- Navigation queries ---

    pub async fn list_docs_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<DocRow>> {
        let rows = sqlx::query_as::<_, DocRow>(
            "SELECT d.id, sf.file_path, d.title
             FROM docs d
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = $1
             ORDER BY sf.file_path",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_doc_content(
        &self,
        snapshot_id: i64,
        file_path: &str,
    ) -> Result<Option<DocContent>> {
        let row = sqlx::query_as::<_, DocContent>(
            "SELECT d.id, sf.file_path, d.title, f.content
             FROM docs d
             JOIN files f ON f.id = d.file_id
             JOIN snapshot_files sf ON sf.file_id = d.file_id
             WHERE sf.snapshot_id = $1 AND sf.file_path = $2",
        )
        .bind(snapshot_id)
        .bind(file_path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_symbols_for_snapshot(
        &self,
        snapshot_id: i64,
        filters: &SymbolFilters,
    ) -> Result<Vec<SymbolRow>> {
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ",
        );
        qb.push_bind(snapshot_id);

        if let Some(ref kind) = filters.kind {
            qb.push(" AND s.kind = ");
            qb.push_bind(kind.clone());
        }
        if let Some(ref visibility) = filters.visibility {
            qb.push(" AND s.visibility = ");
            qb.push_bind(visibility.clone());
        }
        if let Some(ref fp) = filters.file_path {
            qb.push(" AND s.file_path = ");
            qb.push_bind(fp.clone());
        }
        if !filters.include_private {
            qb.push(" AND s.visibility != 'private'");
        }

        qb.push(" ORDER BY s.file_path, s.line_start");

        let rows = qb
            .build_query_as::<SymbolRow>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_symbol_by_id(&self, id: i64) -> Result<Option<SymbolDetail>> {
        let row = sqlx::query_as::<_, SymbolDetail>(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.body, s.parent_id,
                    (SELECT COUNT(*) FROM symbols c WHERE c.parent_id = s.id) AS children_count
             FROM symbols s
             WHERE s.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_symbol_children(&self, parent_id: i64) -> Result<Vec<SymbolRow>> {
        let rows = sqlx::query_as::<_, SymbolRow>(
            "SELECT id, name, qualified_name, kind, visibility, file_path,
                    line_start, line_end, signature, doc_comment, parent_id
             FROM symbols
             WHERE parent_id = $1
             ORDER BY line_start",
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // --- Search support ---

    pub async fn get_file_ids_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT file_id FROM snapshot_files WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    pub async fn get_symbols_for_file(&self, file_id: i64) -> Result<Vec<SymbolRow>> {
        let rows = sqlx::query_as::<_, SymbolRow>(
            "SELECT id, name, qualified_name, kind, visibility, file_path,
                    line_start, line_end, signature, doc_comment, parent_id
             FROM symbols WHERE file_id = $1
             ORDER BY line_start",
        )
        .bind(file_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // --- Refs ---

    pub async fn insert_refs_batch(&self, refs: &[(i64, i64, &str)]) -> Result<usize> {
        if refs.is_empty() {
            return Ok(0);
        }
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO refs (from_symbol_id, to_symbol_id, kind) ",
        );
        qb.push_values(refs, |mut b, (from_id, to_id, kind)| {
            b.push_bind(*from_id);
            b.push_bind(*to_id);
            b.push_bind(*kind);
        });
        qb.push(" ON CONFLICT (from_symbol_id, to_symbol_id, kind) DO NOTHING");
        let result = qb.build().execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    pub async fn refs_exist_for_file(&self, file_id: i64) -> Result<bool> {
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(
                SELECT 1 FROM refs r
                JOIN symbols s ON s.id = r.from_symbol_id
                WHERE s.file_id = $1
            )",
        )
        .bind(file_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    pub async fn get_outbound_refs(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
        kind_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT r.kind AS ref_kind, s.id, s.name, s.qualified_name, s.kind AS sym_kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ",
        );
        qb.push_bind(snapshot_id);
        qb.push(" WHERE r.from_symbol_id = ");
        qb.push_bind(symbol_id);

        if let Some(kind) = kind_filter {
            qb.push(" AND r.kind = ");
            qb.push_bind(kind.to_string());
        }
        qb.push(" ORDER BY s.name LIMIT ");
        qb.push_bind(limit);

        let rows: Vec<RefWithSymbolRow> = qb
            .build_query_as()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn get_inbound_refs(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
        kind_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT r.kind AS ref_kind, s.id, s.name, s.qualified_name, s.kind AS sym_kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = ",
        );
        qb.push_bind(snapshot_id);
        qb.push(" WHERE r.to_symbol_id = ");
        qb.push_bind(symbol_id);

        if let Some(kind) = kind_filter {
            qb.push(" AND r.kind = ");
            qb.push_bind(kind.to_string());
        }
        qb.push(" ORDER BY s.name LIMIT ");
        qb.push_bind(limit);

        let rows: Vec<RefWithSymbolRow> = qb
            .build_query_as()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn get_implementations(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let rows: Vec<RefWithSymbolRow> = sqlx::query_as(
            "SELECT r.kind AS ref_kind, s.id, s.name, s.qualified_name, s.kind AS sym_kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = $1
             WHERE r.to_symbol_id = $2 AND r.kind = 'implements'
             UNION
             SELECT r.kind AS ref_kind, s.id, s.name, s.qualified_name, s.kind AS sym_kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = $1
             WHERE r.from_symbol_id = $2 AND r.kind = 'implements'
             ORDER BY 4",
        )
        .bind(snapshot_id)
        .bind(symbol_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn count_refs_for_symbol(
        &self,
        symbol_id: i64,
        snapshot_id: i64,
    ) -> Result<(i64, i64)> {
        let (inbound,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.from_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = $1
             WHERE r.to_symbol_id = $2",
        )
        .bind(snapshot_id)
        .bind(symbol_id)
        .fetch_one(&self.pool)
        .await?;

        let (outbound,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.to_symbol_id
             JOIN snapshot_files sf ON sf.file_id = s.file_id AND sf.snapshot_id = $1
             WHERE r.from_symbol_id = $2",
        )
        .bind(snapshot_id)
        .bind(symbol_id)
        .fetch_one(&self.pool)
        .await?;

        Ok((inbound, outbound))
    }

    pub async fn get_symbols_with_bodies_for_snapshot(
        &self,
        snapshot_id: i64,
    ) -> Result<Vec<SymbolForRef>> {
        let rows = sqlx::query_as::<_, SymbolForRef>(
            "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.file_path, s.body
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = $1
             ORDER BY s.file_id, s.line_start",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // --- Embeddings ---

    pub async fn embeddings_exist_for_file(&self, file_id: i64) -> Result<bool> {
        let (exists,): (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM embeddings WHERE file_id = $1)")
                .bind(file_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    pub async fn insert_embeddings_batch(&self, embeddings: &[EmbeddingInsert]) -> Result<usize> {
        if embeddings.is_empty() {
            return Ok(0);
        }
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO embeddings (file_id, source_type, source_id, text, vector) ",
        );
        qb.push_values(embeddings, |mut b, e| {
            b.push_bind(e.file_id);
            b.push_bind(&e.source_type);
            b.push_bind(e.source_id);
            b.push_bind(&e.text);
            b.push_bind(e.vector.clone());
        });
        let result = qb.build().execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    pub async fn get_embeddings_for_file_ids(
        &self,
        file_ids: &[i64],
    ) -> Result<Vec<EmbeddingRow>> {
        if file_ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as::<_, EmbeddingRow>(
            "SELECT id, file_id, source_type, source_id, text, vector FROM embeddings WHERE file_id = ANY($1)",
        )
        .bind(file_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn search_embeddings_by_vector(
        &self,
        query_vec: &pgvector::Vector,
        file_ids: &[i64],
        scope: &str,
        limit: i64,
    ) -> Result<Vec<EmbeddingSearchResult>> {
        if file_ids.is_empty() {
            return Ok(vec![]);
        }
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT id, file_id, source_type, source_id, text, 1.0 - (vector <=> ",
        );
        qb.push_bind(query_vec.clone());
        qb.push(") AS score FROM embeddings WHERE file_id = ANY(");
        qb.push_bind(file_ids);
        qb.push(") AND vector IS NOT NULL");

        match scope {
            "docs" => {
                qb.push(" AND source_type = 'doc_chunk'");
            }
            "symbols" => {
                qb.push(" AND source_type = 'symbol'");
            }
            _ => {}
        }

        qb.push(" ORDER BY vector <=> ");
        qb.push_bind(query_vec.clone());
        qb.push(" LIMIT ");
        qb.push_bind(limit);

        let rows: Vec<EmbeddingSearchResult> = qb
            .build_query_as()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    // --- Delete / GC ---

    pub async fn delete_snapshot(&self, snapshot_id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_repo(&self, repo_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM repos WHERE id = $1")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn gc_orphans(&self) -> Result<GcStats> {
        // Delete orphan embeddings (file_id not in any snapshot_files)
        let embeddings_removed = sqlx::query(
            "DELETE FROM embeddings WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        // Delete orphan refs (from/to symbols whose file_id is orphaned)
        let refs_removed = sqlx::query(
            "DELETE FROM refs WHERE from_symbol_id IN (
                SELECT id FROM symbols WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)
            ) OR to_symbol_id IN (
                SELECT id FROM symbols WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)
            )",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        // Delete orphan symbols
        let symbols_removed = sqlx::query(
            "DELETE FROM symbols WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        // Delete orphan docs
        let docs_removed = sqlx::query(
            "DELETE FROM docs WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        // Delete orphan files
        let files_removed = sqlx::query(
            "DELETE FROM files WHERE id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(GcStats {
            files_removed,
            docs_removed,
            symbols_removed,
            refs_removed,
            embeddings_removed,
        })
    }

    pub async fn get_snapshot_files(&self, snapshot_id: i64) -> Result<Vec<SnapshotFileRow>> {
        let rows = sqlx::query_as::<_, SnapshotFileRow>(
            "SELECT file_path, file_id, file_type FROM snapshot_files WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

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
struct RefWithSymbolRow {
    ref_kind: String,
    id: i64,
    name: String,
    qualified_name: String,
    sym_kind: String,
    visibility: String,
    file_path: String,
    line_start: i64,
    line_end: i64,
    signature: String,
    doc_comment: Option<String>,
    parent_id: Option<i64>,
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
