-- LLM-generated summaries for snapshots, modules, and types.
-- Summaries are immutable per snapshot (new snapshot = new summaries).

CREATE TABLE IF NOT EXISTS summaries (
    id          BIGSERIAL PRIMARY KEY,
    snapshot_id BIGINT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    scope       TEXT NOT NULL,          -- "crate", "module:<path>", "type:<symbol_id>"
    content     TEXT NOT NULL,          -- The generated summary text
    model       TEXT NOT NULL,          -- Model used (e.g. "gpt-4", "claude-3-5-sonnet")
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(snapshot_id, scope)
);

CREATE INDEX IF NOT EXISTS idx_summaries_snapshot ON summaries(snapshot_id);
