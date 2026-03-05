-- Persistent repo-level cheatsheet (1 row per repo, live content)
CREATE TABLE IF NOT EXISTS repo_cheatsheets (
    repo_id        TEXT PRIMARY KEY REFERENCES repos(id) ON DELETE CASCADE,
    content        TEXT NOT NULL DEFAULT '',
    snapshot_id    BIGINT REFERENCES snapshots(id) ON DELETE SET NULL,
    model          TEXT NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Append-only patch history (prev + new content for self-contained diffs)
CREATE TABLE IF NOT EXISTS repo_cheatsheet_patches (
    id             BIGSERIAL PRIMARY KEY,
    repo_id        TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    snapshot_id    BIGINT REFERENCES snapshots(id) ON DELETE SET NULL,
    prev_content   TEXT NOT NULL DEFAULT '',
    new_content    TEXT NOT NULL,
    change_summary TEXT NOT NULL DEFAULT '',
    trigger        TEXT NOT NULL DEFAULT 'manual',
    model          TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_cheatsheet_patches_repo
    ON repo_cheatsheet_patches(repo_id, created_at DESC);
