use anyhow::Result;
use super::types::{ConversationRow, ConversationTurnRow};

impl super::Database {
    pub async fn create_conversation(&self, snapshot_id: i64) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO conversations (snapshot_id) VALUES ($1) RETURNING id",
        )
        .bind(snapshot_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_conversation(
        &self,
        conversation_id: i64,
        snapshot_id: i64,
    ) -> Result<Option<ConversationRow>> {
        let row = sqlx::query_as::<_, ConversationRow>(
            "SELECT id, snapshot_id, condensed_context, raw_turn_tokens, condensed_up_to, created_at, updated_at
             FROM conversations WHERE id = $1 AND snapshot_id = $2",
        )
        .bind(conversation_id)
        .bind(snapshot_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_recent_turns(
        &self,
        conversation_id: i64,
        after_index: i32,
        limit: i64,
    ) -> Result<Vec<ConversationTurnRow>> {
        let rows = sqlx::query_as::<_, ConversationTurnRow>(
            "SELECT id, conversation_id, turn_index, question, answer, sources, created_at
             FROM conversation_turns
             WHERE conversation_id = $1 AND turn_index > $2
             ORDER BY turn_index DESC
             LIMIT $3",
        )
        .bind(conversation_id)
        .bind(after_index)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        // Return in chronological order
        let mut rows = rows;
        rows.reverse();
        Ok(rows)
    }

    pub async fn append_turn(
        &self,
        conversation_id: i64,
        question: &str,
        answer: &str,
        sources: &serde_json::Value,
        token_estimate: i32,
    ) -> Result<i32> {
        // Get next turn_index
        let (next_index,): (i32,) = sqlx::query_as(
            "SELECT COALESCE(MAX(turn_index), -1) + 1 FROM conversation_turns WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "INSERT INTO conversation_turns (conversation_id, turn_index, question, answer, sources)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(conversation_id)
        .bind(next_index)
        .bind(question)
        .bind(answer)
        .bind(sources)
        .execute(&self.pool)
        .await?;

        // Update raw_turn_tokens
        sqlx::query(
            "UPDATE conversations SET raw_turn_tokens = raw_turn_tokens + $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(token_estimate)
        .bind(conversation_id)
        .execute(&self.pool)
        .await?;

        Ok(next_index)
    }

    pub async fn update_condensed_context(
        &self,
        conversation_id: i64,
        condensed: &str,
        condensed_up_to: i32,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE conversations SET condensed_context = $1, raw_turn_tokens = 0, condensed_up_to = $2, updated_at = NOW()
             WHERE id = $3",
        )
        .bind(condensed)
        .bind(condensed_up_to)
        .bind(conversation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_conversations(
        &self,
        snapshot_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<ConversationRow>, i64)> {
        let (total,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations WHERE snapshot_id = $1",
        )
        .bind(snapshot_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, ConversationRow>(
            "SELECT id, snapshot_id, condensed_context, raw_turn_tokens, condensed_up_to, created_at, updated_at
             FROM conversations WHERE snapshot_id = $1
             ORDER BY updated_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(snapshot_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows, total))
    }

    pub async fn list_all_turns(
        &self,
        conversation_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<ConversationTurnRow>, i64)> {
        let (total,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_turns WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, ConversationTurnRow>(
            "SELECT id, conversation_id, turn_index, question, answer, sources, created_at
             FROM conversation_turns
             WHERE conversation_id = $1
             ORDER BY turn_index ASC
             LIMIT $2 OFFSET $3",
        )
        .bind(conversation_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows, total))
    }

    pub async fn delete_conversation(
        &self,
        conversation_id: i64,
        snapshot_id: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM conversations WHERE id = $1 AND snapshot_id = $2",
        )
        .bind(conversation_id)
        .bind(snapshot_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
