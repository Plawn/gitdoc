use anyhow::Result;
use super::types::{CheatsheetRow, CheatsheetPatchRow, CheatsheetPatchMeta};

impl super::Database {
    pub async fn get_cheatsheet(&self, repo_id: &str) -> Result<Option<CheatsheetRow>> {
        let row = sqlx::query_as::<_, CheatsheetRow>(
            "SELECT repo_id, content, snapshot_id, model, created_at, updated_at
             FROM repo_cheatsheets WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Transactionally: read prev content, upsert live row, append patch. Returns patch id.
    pub async fn upsert_cheatsheet(
        &self,
        repo_id: &str,
        new_content: &str,
        snapshot_id: Option<i64>,
        change_summary: &str,
        trigger: &str,
        model: &str,
        content_embedding: Option<pgvector::Vector>,
    ) -> Result<i64> {
        let mut tx = self.pool.begin().await?;

        // Read current content (empty string if no row yet)
        let prev_content: String = sqlx::query_scalar(
            "SELECT COALESCE((SELECT content FROM repo_cheatsheets WHERE repo_id = $1), '')",
        )
        .bind(repo_id)
        .fetch_one(&mut *tx)
        .await?;

        // Upsert the live row
        sqlx::query(
            "INSERT INTO repo_cheatsheets (repo_id, content, snapshot_id, model, content_embedding, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT (repo_id)
             DO UPDATE SET content = EXCLUDED.content,
                           snapshot_id = EXCLUDED.snapshot_id,
                           model = EXCLUDED.model,
                           content_embedding = EXCLUDED.content_embedding,
                           updated_at = NOW()",
        )
        .bind(repo_id)
        .bind(new_content)
        .bind(snapshot_id)
        .bind(model)
        .bind(content_embedding)
        .execute(&mut *tx)
        .await?;

        // Append patch
        let (patch_id,): (i64,) = sqlx::query_as(
            "INSERT INTO repo_cheatsheet_patches
                (repo_id, snapshot_id, prev_content, new_content, change_summary, trigger, model)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(repo_id)
        .bind(snapshot_id)
        .bind(&prev_content)
        .bind(new_content)
        .bind(change_summary)
        .bind(trigger)
        .bind(model)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(patch_id)
    }

    pub async fn list_cheatsheet_patches(
        &self,
        repo_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CheatsheetPatchMeta>> {
        let rows = sqlx::query_as::<_, CheatsheetPatchMeta>(
            "SELECT id, repo_id, snapshot_id, change_summary, trigger, model, created_at
             FROM repo_cheatsheet_patches
             WHERE repo_id = $1
             ORDER BY created_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(repo_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_cheatsheet_patch(
        &self,
        repo_id: &str,
        patch_id: i64,
    ) -> Result<Option<CheatsheetPatchRow>> {
        let row = sqlx::query_as::<_, CheatsheetPatchRow>(
            "SELECT id, repo_id, snapshot_id, prev_content, new_content, change_summary, trigger, model, created_at
             FROM repo_cheatsheet_patches
             WHERE repo_id = $1 AND id = $2",
        )
        .bind(repo_id)
        .bind(patch_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}
