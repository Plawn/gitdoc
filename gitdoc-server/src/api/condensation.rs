use crate::db::Database;
use crate::llm_executor::{LlmExecutor, PROMPT_CONDENSATION, PROMPT_CONDENSATION_MERGE, PROMPT_CHEATSHEET_LEARNINGS};

pub(crate) async fn condense_history(
    db: &Database,
    llm: &llm_ai::OpenAiCompatibleClient,
    conversation_id: i64,
    condensed_up_to: i32,
) -> anyhow::Result<()> {
    use llm_ai::{CompletionMessage, Role};

    // Load only un-condensed turns (those after the current boundary)
    let turns = db.list_recent_turns(conversation_id, condensed_up_to, 100).await?;
    if turns.is_empty() {
        return Ok(());
    }

    let max_turn_index = turns.iter().map(|t| t.turn_index).max().unwrap_or(condensed_up_to);

    // Load existing condensed context to merge with
    let existing_condensed = {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT condensed_context FROM conversations WHERE id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&db.pool)
        .await?;
        row.map(|r| r.0).unwrap_or_default()
    };

    // Build the input for the LLM: existing summary + new turns
    let mut input = String::new();
    if !existing_condensed.is_empty() {
        input.push_str("## Existing summary of earlier conversation\n");
        input.push_str(&existing_condensed);
        input.push_str("\n\n## New turns to incorporate\n");
    }
    for turn in &turns {
        input.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    let prompt = if existing_condensed.is_empty() {
        &PROMPT_CONDENSATION
    } else {
        &PROMPT_CONDENSATION_MERGE
    };

    let condense_input_len = input.len();
    tracing::debug!(condense_input_len, input = %input, conversation_id, "condense_history LLM prompt");

    let executor = LlmExecutor::new(llm);
    let user = [CompletionMessage::new(Role::User, &input)];
    let resp = executor.run_anyhow(prompt, &user).await?;

    tracing::debug!(condense_output_len = resp.content.len(), output = %resp.content, conversation_id, "condense_history LLM response");

    db.update_condensed_context(conversation_id, &resp.content, max_turn_index).await?;
    tracing::info!(
        conversation_id,
        condensed_up_to = max_turn_index,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        turns_condensed = turns.len(),
        "conversation history condensed"
    );
    Ok(())
}

pub(crate) async fn update_cheatsheet_from_conversation(
    db: &Database,
    llm_client: &llm_ai::OpenAiCompatibleClient,
    snapshot_id: i64,
    turns: &[crate::db::ConversationTurnRow],
) -> anyhow::Result<()> {
    // Get repo_id from snapshot
    let snapshot = db.get_snapshot(snapshot_id).await?
        .ok_or_else(|| anyhow::anyhow!("snapshot {snapshot_id} not found"))?;
    let repo_id = &snapshot.repo_id;

    // Only update if a cheatsheet already exists (don't auto-generate initial)
    let existing = db.get_cheatsheet(repo_id).await?;
    if existing.is_none() {
        return Ok(());
    }

    // Build history string from turns
    let mut history = String::new();
    for turn in turns {
        history.push_str(&format!("Q: {}\nA: {}\n\n", turn.question, turn.answer));
    }

    // Extract learnings
    use llm_ai::{CompletionMessage, Role};
    let executor = LlmExecutor::new(llm_client);
    let user = [CompletionMessage::new(Role::User, &history)];
    let resp = executor.run_anyhow(&PROMPT_CHEATSHEET_LEARNINGS, &user).await?;

    tracing::info!(
        repo_id,
        input_tokens = resp.input_tokens,
        output_tokens = resp.output_tokens,
        "conversation learnings extracted"
    );

    let learnings = resp.content.trim();
    if learnings.is_empty() || learnings == "No significant learnings." {
        tracing::debug!(repo_id, "no significant learnings from conversation");
        return Ok(());
    }

    // Update cheatsheet with learnings
    let cs = existing.unwrap();
    let (new_content, change_summary) =
        crate::cheatsheet::update_cheatsheet_from_learnings(llm_client, &cs.content, learnings).await?;

    let model_name = llm_client.name();
    db.upsert_cheatsheet(repo_id, &new_content, Some(snapshot_id), &change_summary, "conversation_reset", model_name, None)
        .await?;

    tracing::info!(repo_id, "cheatsheet updated from conversation learnings");
    Ok(())
}
