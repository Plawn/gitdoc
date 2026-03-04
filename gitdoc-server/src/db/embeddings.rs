use anyhow::Result;
use sqlx::QueryBuilder;
use super::types::{EmbeddingInsert, EmbeddingRow, EmbeddingSearchResult};

impl super::Database {
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
}
