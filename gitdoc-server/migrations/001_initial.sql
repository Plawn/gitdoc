CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE repos (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE snapshots (
    id          BIGSERIAL PRIMARY KEY,
    repo_id     TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    commit_sha  TEXT NOT NULL,
    label       TEXT,
    indexed_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    status      TEXT NOT NULL DEFAULT 'indexing',
    stats       TEXT,
    UNIQUE(repo_id, commit_sha)
);

CREATE TABLE files (
    id          BIGSERIAL PRIMARY KEY,
    checksum    TEXT NOT NULL UNIQUE,
    content     TEXT
);

CREATE TABLE snapshot_files (
    snapshot_id BIGINT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    file_path   TEXT NOT NULL,
    file_id     BIGINT NOT NULL REFERENCES files(id),
    file_type   TEXT NOT NULL DEFAULT 'other',
    PRIMARY KEY (snapshot_id, file_path)
);

CREATE TABLE docs (
    id          BIGSERIAL PRIMARY KEY,
    file_id     BIGINT NOT NULL REFERENCES files(id),
    title       TEXT
);

CREATE TABLE symbols (
    id              BIGSERIAL PRIMARY KEY,
    file_id         BIGINT NOT NULL REFERENCES files(id),
    name            TEXT NOT NULL,
    qualified_name  TEXT NOT NULL,
    kind            TEXT NOT NULL,
    visibility      TEXT NOT NULL DEFAULT 'private',
    file_path       TEXT NOT NULL,
    line_start      BIGINT NOT NULL,
    line_end        BIGINT NOT NULL,
    byte_start      BIGINT NOT NULL,
    byte_end        BIGINT NOT NULL,
    parent_id       BIGINT REFERENCES symbols(id),
    signature       TEXT NOT NULL,
    doc_comment     TEXT,
    body            TEXT NOT NULL
);

CREATE INDEX idx_symbols_file_id ON symbols(file_id);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_kind ON symbols(kind);

CREATE TABLE refs (
    id              BIGSERIAL PRIMARY KEY,
    from_symbol_id  BIGINT NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    to_symbol_id    BIGINT NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL
);

CREATE INDEX idx_refs_from ON refs(from_symbol_id);
CREATE INDEX idx_refs_to ON refs(to_symbol_id);
CREATE UNIQUE INDEX idx_refs_unique ON refs(from_symbol_id, to_symbol_id, kind);

CREATE TABLE embeddings (
    id          BIGSERIAL PRIMARY KEY,
    file_id     BIGINT NOT NULL,
    source_type TEXT NOT NULL,
    source_id   BIGINT NOT NULL,
    text        TEXT NOT NULL,
    vector      vector
);

CREATE INDEX idx_embeddings_file_id ON embeddings(file_id);
CREATE INDEX idx_embeddings_source ON embeddings(source_type, source_id);
