use anyhow::Result;
use super::types::{SnapshotFileRow, SnapshotFileInfo};

impl super::Database {
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

    pub async fn get_file_ids_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT file_id FROM snapshot_files WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
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

    pub async fn get_snapshot_files(&self, snapshot_id: i64) -> Result<Vec<SnapshotFileRow>> {
        let rows = sqlx::query_as::<_, SnapshotFileRow>(
            "SELECT file_path, file_id, file_type FROM snapshot_files WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_snapshot_file_paths(
        &self,
        snapshot_id: i64,
    ) -> Result<Vec<SnapshotFileInfo>> {
        let rows = sqlx::query_as::<_, SnapshotFileInfo>(
            "SELECT sf.file_path, sf.file_type,
                    (SELECT COUNT(*) FROM symbols s WHERE s.file_id = sf.file_id AND s.visibility != 'private') AS public_symbol_count
             FROM snapshot_files sf
             WHERE sf.snapshot_id = $1
             ORDER BY sf.file_path",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
