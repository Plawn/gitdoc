use anyhow::Result;
use super::types::SnapshotRow;

impl super::Database {
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

    pub async fn delete_snapshot(&self, snapshot_id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
