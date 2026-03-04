# GitDoc — Progress Tracker

## Phase 0 — Bootstrap ✅
- [x] Workspace Cargo with `gitdoc-server` and `gitdoc-mcp`
- [x] Server: axum minimal, `GET /health` → 200
- [x] MCP: server minimal with tool `ping` → calls `/health` → "pong"
- [x] End-to-end round-trip works

## Phase 1 — Schema & indexation basique ✅
- [x] Schema SQLite (8 tables: repos, snapshots, files, snapshot_files, docs, symbols, refs, embeddings)
- [x] Git Walker (gix): resolve ref, walk tree, SHA-256 checksums
- [x] Doc Parser: extract markdown title
- [x] Tree-sitter Rust: function, struct, enum, trait, impl, type_alias, const, static, mod, macro
- [x] Tree-sitter TypeScript/JS: function, class, interface, type_alias, enum, export
- [x] Pipeline: git walk → classify → deduplicate via checksum → parse → store → finalize
- [x] Routes API: `POST /repos`, `GET /repos`, `GET /repos/:id`, `POST /repos/:id/index`
- [x] Multi-commit deduplication verified (unchanged files share same file_id)
- [x] Tests: 40 tests (unit + integration + ArcRun real repo)

## Phase 2 — Tools MCP de navigation ✅
- [x] Routes API: `/snapshots/:id/overview`, docs, symbols
- [x] Tools MCP: `list_repos`, `get_repo_overview`, `index_repo`, `list_docs`, `read_doc`, `list_symbols`, `get_symbol`
- [x] Snapshot resolver in MCP: ref → snapshot_id
- [x] Tests: 49 tests pass

## Phase 3 — Graphe de références ✅
- [x] Import resolver: regex-based `use`/`import` parsing, resolution table
- [x] Body scanner: word-token extraction, keyword filtering, identifier resolution
- [x] Structural relations: `impl Trait for Struct`, `extends`, `implements`
- [x] DB layer: indexes on refs, 8 new methods (batch insert, inbound/outbound/implementations queries, ref counts)
- [x] Routes API: `/snapshots/:id/symbols/:id/references`, `/snapshots/:id/symbols/:id/implementations`
- [x] Tools MCP: `find_references`, `get_dependencies`, `get_implementations`
- [x] Tests: 49 tests pass (8 new reference resolver unit tests)

## Phase 4 — Full-text search (Tantivy) ✅
- [x] Tantivy index for docs (content + title + file_id) with snippet generation
- [x] Tantivy index for symbols (name, signature, doc_comment + file_id) with kind/visibility filters
- [x] Snapshot filtering via BooleanQuery on file_id (fetched from snapshot_files)
- [x] Routes API: `GET /snapshots/:id/search/docs`, `GET /snapshots/:id/search/symbols`
- [x] Tools MCP: `search_docs`, `search_symbols`
- [x] Tests: 55 tests pass (4 new search unit tests, 2 new search integration tests)

## Phase 5 — Recherche sémantique ✅
- [x] Trait `EmbeddingProvider` with sync `ureq` HTTP + two providers: `CohereProvider` (embed-v4.0, search_document/search_query input types) and `OpenAiProvider` (text-embedding-3-small)
- [x] Config: `EmbeddingConfig` reads `COHERE_KEY` or `OPENAI_API_KEY` from env; embeddings silently skipped if no key set
- [x] DB: `file_id` column added to embeddings table + indexes; methods `embeddings_exist_for_file`, `insert_embeddings_batch`, `get_embeddings_for_file_ids`; types `EmbeddingInsert`, `EmbeddingRow`
- [x] Doc chunking: split markdown at `##` headings, flush at ~2000 chars; `DocChunk { section_title, text }`
- [x] Symbol embedding: `{kind} {name}: {signature}\n{doc_comment}` format
- [x] Pipeline integration: collect pending embeddings during doc/symbol parsing, batch embed (96 per API call), dedup via `embeddings_exist_for_file`, report `embeddings_count` in stats
- [x] Helpers: `cosine_similarity`, `vec_to_blob`/`blob_to_vec` (little-endian f32 serialization), `create_provider` factory
- [x] Route API: `GET /snapshots/:id/search/semantic?q=...&scope=all|docs|symbols&limit=10` — brute-force cosine similarity with metadata enrichment; returns 503 if no provider
- [x] Tool MCP: `semantic_search` with params `repo_id, ref?, query, scope?, limit?`
- [x] Tests: 68 tests pass (5 embedding unit tests, 4 doc chunking tests, 4 embedding integration tests with mock provider)

## Phase 6 — Diff cross-version ✅
- [x] Diff logic: fetch symbols for two snapshots, index by `qualified_name`, categorize into added/removed/modified (signature or visibility changes)
- [x] Route API: `GET /snapshots/:from_id/diff/:to_id?kind=...&include_private=false`
- [x] Response: `{ from_snapshot, to_snapshot, added[], removed[], modified[], summary }` with `changes` array and `from`/`to` fields on modified entries
- [x] Client: `DiffResponse`, `DiffSymbolEntry`, `ModifiedSymbol`, `ModifiedFields`, `DiffSummary` types + `diff_symbols()` method
- [x] Fixed MCP client timestamp types: `RepoRow.created_at` and `SnapshotRow.indexed_at` from `i64` → `String` (ISO 8601)
- [x] Tool MCP: `diff_symbols` with params `repo_id, from_ref?, to_ref?, kind?, include_private?` — resolves refs via snapshot resolver
- [x] Integration test: indexes v1, modifies code (changed signature + removed function + added function), indexes v2, verifies diff correctness

## Phase 7 — Polish & robustesse ⬜
- [ ] Error handling with actionable messages
- [ ] TOML config (port, DB path, embedding provider, exclusion patterns)
- [ ] Default exclusion patterns (`node_modules/`, `target/`, `.git/`, `vendor/`)
- [ ] Structured tracing
- [ ] Integration tests with fixtures
- [ ] Garbage collection for orphan files and deleted snapshots
