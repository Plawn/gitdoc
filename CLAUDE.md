# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is GitDoc

A code intelligence server for LLM agents. LLMs navigate codebases through structured data (symbols, docs, references, embeddings) extracted by tree-sitter, rather than reading raw source files. Uses PostgreSQL+pgvector for storage, Tantivy for full-text search, and gix for git operations.

## Architecture

Three-crate Rust workspace:

- **gitdoc-server** — Axum HTTP server (shared daemon). Indexes repos, serves all clients. Owns the DB, search indexes, and embedding/LLM integrations. `AppState` in `src/lib.rs` holds all shared state (`Database`, `SearchIndex`, `EmbeddingProvider`, `LlmClient`, `Config`).
- **gitdoc-mcp** — Thin MCP proxy (one per agent session, stdio transport via `rmcp`). Stateless except per-session conversation tracking. Talks to gitdoc-server over HTTP. Uses `mcp-framework` (custom fork) with `CapabilityFilter` for mode-based tool filtering.
- **llm-ai** — Generic OpenAI-compatible LLM client library. Supports Azure OpenAI, Azure Inference, and Ollama. Used by gitdoc-server for summaries, cheatsheets, and conversational Q&A.

### Key server modules

- `db/` — SQLx-based PostgreSQL layer. Sub-modules per domain (repos, snapshots, files, symbols, docs, refs, embeddings, summaries, conversations, cheatsheets, architect). Migrations in `migrations/` run automatically on startup.
- `indexer/` — Pipeline: git walk → tree-sitter parse → reference resolution → embedding. `pipeline.rs` orchestrates the 8-step flow. Language grammars in `indexer/languages/`.
- `api/` — Axum route handlers. Each file maps to a REST resource group (repos, snapshots, symbols, search, converse, cheatsheet, architect, etc.). Routes assembled in `api/mod.rs`.
- `search.rs` — Tantivy full-text index management.
- `embeddings.rs` — Cohere and OpenAI embedding providers.
- `cheatsheet.rs` — LLM-generated persistent repo knowledge that accumulates across sessions.
- `architect.rs` — Technology advisor: library profiles + stack rules knowledge base.

### MCP server design

- `server.rs` — All MCP tool handlers via `#[tool_router]` macro. Tools annotated with `#[tool]`.
- `mode_filter.rs` — Filters tool list based on `McpMode` (Simple=8 tools, Granular=31 tools).
- `snapshot_resolver.rs` — Resolves `repo_id` + optional `ref` to a snapshot ID, auto-indexing if needed.
- `params.rs` — Deserializable parameter structs for each MCP tool.

## Build & Development Commands

Uses `just` (justfile) as task runner. The justfile loads `.env` automatically.

```sh
just check              # cargo check --workspace
just build              # cargo build --workspace
just build-release      # cargo build --workspace --release
just test               # unit tests only (cargo test --workspace --lib)
just test-all           # unit + integration tests
just test-integration   # integration tests only (-p gitdoc-server --test integration)
```

Run a single test:
```sh
cargo test -p gitdoc-server --lib -- test_name
cargo test -p gitdoc-server --test integration -- test_name
```

### Database setup

```sh
just db-up              # docker compose up -d postgres (pgvector/pgvector:pg17, port 5433)
just db-down            # docker compose down
```

Docker Compose exposes PostgreSQL on port **5433** (not 5432). Credentials: `gitdoc:gitdoc@localhost:5433/gitdoc`.

### Running

```sh
just start              # db-up + run-server
just run-server         # server with docker-compose postgres URL
just run-mcp            # MCP proxy (stdio)
```

Integration tests use testcontainers — they spin up their own PostgreSQL instance, no manual DB needed.

## Environment Variables

Server: `GITDOC_DATABASE_URL`, `GITDOC_BIND_ADDR` (default 127.0.0.1:3000), `GITDOC_INDEX_PATH`, `COHERE_KEY` or `OPENAI_API_KEY` for embeddings, `GITDOC_LLM_*` for LLM config. Can also use `gitdoc.toml`.

MCP: `GITDOC_SERVER_URL` (default http://127.0.0.1:3000), `GITDOC_MCP_MODE` (`simple`|`granular`).

## Supported Languages

Rust (`.rs`), TypeScript (`.ts`, `.tsx`), JavaScript (`.js`, `.jsx`), Markdown (`.md`, `.mdx`).

## Conventions

- Rust edition 2024 for gitdoc-server and gitdoc-mcp, 2021 for llm-ai
- SQLx with raw SQL queries (no ORM) — migration files are numbered sequentially (`001_initial.sql`, etc.)
- Error handling: `anyhow::Result` throughout server, `thiserror` in llm-ai
- Async runtime: Tokio
- The `gitdoc_repos/` directory contains cloned repos used for indexing (and vendored dependencies like mcp-framework) — don't modify these
