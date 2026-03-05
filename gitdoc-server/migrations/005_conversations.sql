-- Conversational mode: multi-turn Q&A sessions tied to a snapshot.

CREATE TABLE IF NOT EXISTS conversations (
    id                BIGSERIAL PRIMARY KEY,
    snapshot_id       BIGINT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    condensed_context TEXT NOT NULL DEFAULT '',
    raw_turn_tokens   INT NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_conversations_snapshot ON conversations(snapshot_id);

CREATE TABLE IF NOT EXISTS conversation_turns (
    id              BIGSERIAL PRIMARY KEY,
    conversation_id BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    turn_index      INT NOT NULL,
    question        TEXT NOT NULL,
    answer          TEXT NOT NULL,
    sources         JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(conversation_id, turn_index)
);

CREATE INDEX IF NOT EXISTS idx_conversation_turns_conversation ON conversation_turns(conversation_id);
