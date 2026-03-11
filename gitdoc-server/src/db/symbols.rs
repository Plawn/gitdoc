use anyhow::Result;
use sqlx::QueryBuilder;
use super::types::*;

impl super::Database {
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

    pub async fn get_public_api_symbols(
        &self,
        snapshot_id: i64,
        module_path: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PublicApiSymbol>> {
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ",
        );
        qb.push_bind(snapshot_id);
        qb.push(" AND s.visibility != 'private'");

        if let Some(mp) = module_path {
            let pattern = format!("%{}%", mp.replace("::", "/"));
            qb.push(" AND s.file_path LIKE ");
            qb.push_bind(pattern);
        }

        qb.push(" ORDER BY s.file_path, s.line_start");
        qb.push(" LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        let rows = qb
            .build_query_as::<PublicApiSymbol>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_module_symbols(
        &self,
        snapshot_id: i64,
    ) -> Result<Vec<ModuleSymbol>> {
        let rows = sqlx::query_as::<_, ModuleSymbol>(
            "SELECT s.id, s.name, s.qualified_name, s.file_path, s.doc_comment, s.parent_id
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = $1 AND s.kind = 'module'
             ORDER BY s.file_path, s.line_start",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_symbols_by_ids(&self, ids: &[i64]) -> Result<Vec<SymbolDetail>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as::<_, SymbolDetail>(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.body, s.parent_id,
                    (SELECT COUNT(*) FROM symbols c WHERE c.parent_id = s.id) AS children_count
             FROM symbols s
             WHERE s.id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_symbols_with_body_for_snapshot_by_qnames(
        &self,
        snapshot_id: i64,
        qualified_names: &[&str],
    ) -> Result<Vec<SymbolDetail>> {
        if qualified_names.is_empty() {
            return Ok(vec![]);
        }
        let qnames: Vec<String> = qualified_names.iter().map(|s| s.to_string()).collect();
        let rows = sqlx::query_as::<_, SymbolDetail>(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.body, s.parent_id,
                    (SELECT COUNT(*) FROM symbols c WHERE c.parent_id = s.id) AS children_count
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = $1 AND s.qualified_name = ANY($2)",
        )
        .bind(snapshot_id)
        .bind(&qnames)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_public_signatures_by_file(
        &self,
        snapshot_id: i64,
        file_paths: &[String],
    ) -> Result<Vec<SymbolRow>> {
        if file_paths.is_empty() {
            return Ok(vec![]);
        }
        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.visibility, s.file_path,
                    s.line_start, s.line_end, s.signature, s.doc_comment, s.parent_id
             FROM symbols s
             JOIN snapshot_files sf ON sf.file_id = s.file_id
             WHERE sf.snapshot_id = ",
        );
        qb.push_bind(snapshot_id);
        qb.push(" AND s.visibility != 'private' AND s.file_path = ANY(");
        qb.push_bind(file_paths);
        qb.push(") ORDER BY s.file_path, s.line_start");

        let rows = qb
            .build_query_as::<SymbolRow>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }
}
