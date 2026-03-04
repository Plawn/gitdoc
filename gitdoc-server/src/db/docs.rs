use anyhow::Result;
use super::types::{DocRow, DocContent};

impl super::Database {
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
}
