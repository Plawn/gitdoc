use anyhow::Result;
use super::types::SummaryRow;

impl super::Database {
    pub async fn upsert_summary(
        &self,
        snapshot_id: i64,
        scope: &str,
        content: &str,
        model: &str,
    ) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO summaries (snapshot_id, scope, content, model)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (snapshot_id, scope)
             DO UPDATE SET content = EXCLUDED.content, model = EXCLUDED.model, created_at = NOW()
             RETURNING id",
        )
        .bind(snapshot_id)
        .bind(scope)
        .bind(content)
        .bind(model)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_summary(
        &self,
        snapshot_id: i64,
        scope: &str,
    ) -> Result<Option<SummaryRow>> {
        let row = sqlx::query_as::<_, SummaryRow>(
            "SELECT id, snapshot_id, scope, content, model, created_at
             FROM summaries WHERE snapshot_id = $1 AND scope = $2",
        )
        .bind(snapshot_id)
        .bind(scope)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_summaries(&self, snapshot_id: i64) -> Result<Vec<SummaryRow>> {
        let rows = sqlx::query_as::<_, SummaryRow>(
            "SELECT id, snapshot_id, scope, content, model, created_at
             FROM summaries WHERE snapshot_id = $1 ORDER BY scope",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
