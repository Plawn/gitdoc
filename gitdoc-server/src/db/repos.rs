use anyhow::Result;
use super::types::{RepoRow, RepoSummaryRow};

impl super::Database {
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

    pub async fn list_repos(&self) -> Result<Vec<RepoSummaryRow>> {
        let rows = sqlx::query_as::<_, RepoSummaryRow>(
            "SELECT r.id, r.path, r.name, r.url, r.created_at,
                    COALESCE(s.cnt, 0) AS snapshot_count,
                    ls.label AS latest_snapshot_label,
                    ls.commit_sha AS latest_snapshot_commit,
                    ls.indexed_at AS latest_snapshot_indexed_at
             FROM repos r
             LEFT JOIN LATERAL (
                 SELECT COUNT(*) AS cnt FROM snapshots WHERE repo_id = r.id
             ) s ON true
             LEFT JOIN LATERAL (
                 SELECT label, commit_sha, indexed_at
                 FROM snapshots WHERE repo_id = r.id
                 ORDER BY indexed_at DESC LIMIT 1
             ) ls ON true
             ORDER BY r.created_at DESC",
        )
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

    pub async fn delete_repo(&self, repo_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM repos WHERE id = $1")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
