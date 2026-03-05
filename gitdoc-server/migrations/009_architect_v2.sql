-- Phase 1: Cheatsheet embeddings for architect search
ALTER TABLE repo_cheatsheets ADD COLUMN IF NOT EXISTS content_embedding VECTOR(1024);

-- Phase 2: Project profiles
CREATE TABLE IF NOT EXISTS project_profiles (
    id                TEXT PRIMARY KEY,
    repo_id           TEXT REFERENCES repos(id) ON DELETE SET NULL,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    stack             JSONB NOT NULL DEFAULT '[]',
    constraints       TEXT NOT NULL DEFAULT '',
    code_style        TEXT NOT NULL DEFAULT '',
    content_embedding VECTOR(1024),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE stack_rules ADD COLUMN IF NOT EXISTS project_profile_id TEXT
    REFERENCES project_profiles(id) ON DELETE SET NULL;

-- Phase 3: Architecture decisions
CREATE TABLE IF NOT EXISTS arch_decisions (
    id                  BIGSERIAL PRIMARY KEY,
    project_profile_id  TEXT REFERENCES project_profiles(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    context             TEXT NOT NULL DEFAULT '',
    choice              TEXT NOT NULL,
    alternatives        TEXT NOT NULL DEFAULT '',
    reasoning           TEXT NOT NULL DEFAULT '',
    outcome             TEXT,
    status              TEXT NOT NULL DEFAULT 'active',
    content_embedding   VECTOR(1024),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_arch_decisions_project ON arch_decisions(project_profile_id);
CREATE INDEX IF NOT EXISTS idx_arch_decisions_status ON arch_decisions(status);

-- Phase 5: Pattern library
CREATE TABLE IF NOT EXISTS arch_patterns (
    id                BIGSERIAL PRIMARY KEY,
    name              TEXT NOT NULL,
    category          TEXT NOT NULL DEFAULT '',
    description       TEXT NOT NULL DEFAULT '',
    libs_involved     TEXT[] NOT NULL DEFAULT '{}',
    pattern_text      TEXT NOT NULL,
    content_embedding VECTOR(1024),
    source            TEXT NOT NULL DEFAULT 'manual',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_arch_patterns_category ON arch_patterns(category);
