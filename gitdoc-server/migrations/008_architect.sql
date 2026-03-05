CREATE TABLE IF NOT EXISTS lib_profiles (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    repo_id           TEXT REFERENCES repos(id) ON DELETE SET NULL,
    category          TEXT NOT NULL DEFAULT '',
    version_hint      TEXT NOT NULL DEFAULT '',
    profile           TEXT NOT NULL DEFAULT '',
    profile_embedding VECTOR(1024),
    source            TEXT NOT NULL DEFAULT 'auto',
    model             TEXT NOT NULL DEFAULT '',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_lib_profiles_category ON lib_profiles(category);

CREATE TABLE IF NOT EXISTS stack_rules (
    id                BIGSERIAL PRIMARY KEY,
    rule_type         TEXT NOT NULL,
    subject           TEXT NOT NULL,
    content           TEXT NOT NULL,
    lib_profile_id    TEXT REFERENCES lib_profiles(id) ON DELETE SET NULL,
    priority          INT NOT NULL DEFAULT 0,
    content_embedding VECTOR(1024),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_stack_rules_type ON stack_rules(rule_type);
