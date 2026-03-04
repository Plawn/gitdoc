use anyhow::Result;
use sqlx::QueryBuilder;
use super::types::{RefWithSymbol, RefWithSymbolRow};

impl super::Database {
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
}
