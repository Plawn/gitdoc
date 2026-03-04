use anyhow::Result;
use super::types::GcStats;

impl super::Database {
    pub async fn gc_orphans(&self) -> Result<GcStats> {
        let embeddings_removed = sqlx::query(
            "DELETE FROM embeddings WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

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

        let symbols_removed = sqlx::query(
            "DELETE FROM symbols WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        let docs_removed = sqlx::query(
            "DELETE FROM docs WHERE file_id NOT IN (SELECT DISTINCT file_id FROM snapshot_files)",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

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
}
