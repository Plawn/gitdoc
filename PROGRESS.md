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

## Phase 2 — Tools MCP de navigation ⬜
- [ ] Routes API: `/repos/:id/snapshots`, `/snapshots/:id/overview`, docs, symbols
- [ ] Tools MCP: `list_repos`, `get_repo_overview`, `index_repo`, `list_docs`, `read_doc`, `list_symbols`, `get_symbol`
- [ ] Snapshot resolver in MCP: ref → snapshot_id
- [ ] Test with Claude Code

## Phase 3 — Graphe de références ⬜
- [ ] Import resolver: parse `use`/`import`, resolution table
- [ ] Body scanner: identify referenced identifiers
- [ ] Structural relations: `impl Trait for Struct`, `extends`, `implements`
- [ ] Routes API: references with direction inbound/outbound, implementations
- [ ] Tools MCP: `find_references`, `get_dependencies`, `get_implementations`

## Phase 4 — Full-text search (Tantivy) ⬜
- [ ] Tantivy index for docs (content + file_id)
- [ ] Tantivy index for symbols (name, signature, doc_comment + file_id)
- [ ] Snapshot filtering via snapshot_files join at query time
- [ ] Routes API + Tools MCP: `search_docs`, `search_symbols`

## Phase 5 — Recherche sémantique ⬜
- [ ] Trait `EmbeddingProvider` + Cohere implementation (using `COHERE_KEY`)
- [ ] Doc chunking: split markdown by sections (~500 tokens)
- [ ] Symbol embedding: `{kind} {name}: {signature}\n{doc_comment}`
- [ ] Storage BLOB + cosine similarity filtered by snapshot
- [ ] Route API + Tool MCP: `semantic_search`

## Phase 6 — Diff cross-version ⬜
- [ ] Diff logic: compare public symbols by qualified_name between snapshots
- [ ] Route API + Tool MCP: `diff_symbols`

## Phase 7 — Polish & robustesse ⬜
- [ ] Error handling with actionable messages
- [ ] TOML config (port, DB path, embedding provider, exclusion patterns)
- [ ] Default exclusion patterns (`node_modules/`, `target/`, `.git/`, `vendor/`)
- [ ] Structured tracing
- [ ] Integration tests with fixtures
- [ ] Garbage collection for orphan files and deleted snapshots
