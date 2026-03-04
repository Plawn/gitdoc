# GitDoc

A code intelligence server for LLM agents. The core idea: **the LLM never reads raw source files**. Instead, it navigates codebases through structured data extracted by tree-sitter — symbols, docs, references, and embeddings.

This eliminates noise (dead code, boilerplate, config files) and lets agents enter a repo through documentation ("the what") and drill into symbols ("the how") via a dependency graph.

## Architecture

```
┌──────────────┐     stdio/MCP      ┌──────────────┐      HTTP       ┌──────────────────┐
│  Claude Code  │ ◄──────────────► │  gitdoc-mcp  │ ◄────────────► │  gitdoc-server   │
│  (or any MCP  │                   │  (per-session │                │  (shared daemon)  │
│   client)     │                   │   proxy)      │                │                   │
└──────────────┘                   └──────────────┘                ├──────────────────┤
                                                                    │ PostgreSQL+pgvector│
                                                                    │ Tantivy FTS index  │
                                                                    │ gix (git)          │
                                                                    │ tree-sitter        │
                                                                    └──────────────────┘
```

- **gitdoc-server** — runs permanently, indexes repos, serves all clients
- **gitdoc-mcp** — thin MCP proxy launched per agent session; no state of its own

Multiple agents share the same indexed data without re-indexing.

## Features

- **Git-native snapshots** — index any commit/ref; compare public APIs across versions with `diff_symbols`
- **Tree-sitter symbol extraction** — functions, structs, traits, classes, interfaces, enums, and more
- **Cross-reference graph** — calls, imports, type refs, implements, extends, field access
- **Documentation parsing** — markdown files chunked at `##` boundaries with title extraction
- **Full-text search** — Tantivy indexes over docs and symbols
- **Semantic search** — pgvector cosine similarity via Cohere or OpenAI embeddings
- **File deduplication** — SHA-256 content addressing; unchanged files across commits share all parsed data
- **14 MCP tools** — everything an agent needs to understand a codebase without reading raw files

### Supported Languages

| Language | Extensions |
|----------|-----------|
| Rust | `.rs` |
| TypeScript | `.ts`, `.tsx` (excludes `.d.ts`) |
| JavaScript | `.js`, `.jsx` |
| Markdown | `.md`, `.mdx` |

## Prerequisites

- Rust toolchain (stable)
- PostgreSQL with the [pgvector](https://github.com/pgvector/pgvector) extension
- (Optional) Cohere or OpenAI API key for semantic search

## Setup

### 1. Database

Create a PostgreSQL database. The server runs migrations automatically on startup.

```sh
createdb gitdoc
# Ensure pgvector is available:
psql gitdoc -c 'CREATE EXTENSION IF NOT EXISTS vector;'
```

### 2. Build

```sh
cargo build --release
```

### 3. Run the server

```sh
GITDOC_DATABASE_URL=postgres://localhost/gitdoc \
GITDOC_BIND_ADDR=127.0.0.1:3000 \
GITDOC_INDEX_PATH=./gitdoc_index \
cargo run --bin gitdoc-server
```

### 4. Run the MCP client

```sh
GITDOC_SERVER_URL=http://127.0.0.1:3000 cargo run --bin gitdoc-mcp
```

Or add it to your Claude Code MCP config:

```json
{
  "mcpServers": {
    "gitdoc": {
      "command": "cargo",
      "args": ["run", "--bin", "gitdoc-mcp"],
      "cwd": "/path/to/gitdoc",
      "env": {
        "GITDOC_SERVER_URL": "http://127.0.0.1:3000"
      }
    }
  }
}
```

## Configuration

### gitdoc-server

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `GITDOC_BIND_ADDR` | `127.0.0.1:3000` | TCP listen address |
| `GITDOC_DATABASE_URL` | `postgres://localhost/gitdoc` | PostgreSQL connection URL |
| `GITDOC_INDEX_PATH` | `./gitdoc_index` | Directory for Tantivy full-text indexes |
| `COHERE_KEY` | — | Cohere API key (`embed-v4.0`, 1024-dim) |
| `OPENAI_API_KEY` | — | OpenAI API key (`text-embedding-3-small`, 1536-dim) |
| `RUST_LOG` | `info` | Tracing filter directive |

If neither embedding key is set, semantic search returns `503 Service Unavailable`. When both are set, Cohere takes precedence.

### gitdoc-mcp

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `GITDOC_SERVER_URL` | `http://127.0.0.1:3000` | URL of the gitdoc-server instance |

## API Reference

### Repos

```
POST /repos                         Create a repo  { id, path, name }
GET  /repos                         List all repos
GET  /repos/:repo_id                Get repo with its snapshots
POST /repos/:repo_id/index          Trigger indexing  { commit?, label? }
```

### Snapshot Navigation

```
GET  /snapshots/:id/overview        README + doc tree + top-level symbols
GET  /snapshots/:id/docs            List all markdown files
GET  /snapshots/:id/docs/*path      Read full markdown content
GET  /snapshots/:id/symbols         List symbols  ?kind=&visibility=&file_path=&include_private=
GET  /snapshots/:id/symbols/:sym_id Symbol detail with children and ref counts
```

### References

```
GET  /snapshots/:id/symbols/:sym_id/references        ?direction=inbound|outbound&kind=&limit=
GET  /snapshots/:id/symbols/:sym_id/implementations
```

### Diff

```
GET  /snapshots/:from_id/diff/:to_id   ?kind=&include_private=
```

Returns added, removed, and modified symbols between two snapshots.

### Search

```
GET  /snapshots/:id/search/docs        ?q=&limit=           Full-text (Tantivy)
GET  /snapshots/:id/search/symbols     ?q=&kind=&visibility=&limit=  Full-text (Tantivy)
GET  /snapshots/:id/search/semantic    ?q=&scope=all|docs|symbols&limit=  Embeddings (pgvector)
```

### Health

```
GET  /health    → "ok"
```

## MCP Tools

The MCP server exposes 19 tools to LLM agents. Most tools accept `repo_id` + optional `ref`. If `ref` is omitted, the most recent snapshot is used.

### Setup & Discovery
| Tool | Description |
|------|-------------|
| `ping` | Health check — verify server is reachable |
| `list_repos` | List all registered repos with IDs, names, paths |
| `register_repo` | Register a repo (`url` for remote, `path` for local) |
| `fetch_repo` | Pull latest changes for a URL-cloned repo |
| `index_repo` | Create a snapshot at a commit (**required** before querying) |

### Browsing
| Tool | Description |
|------|-------------|
| `get_repo_overview` | README + doc tree + top-level public symbols (start here) |
| `list_docs` | List markdown/text files in a snapshot |
| `read_doc` | Read full content of a doc file |
| `list_symbols` | List code symbols with filters (kind, file_path, include_private) |
| `get_symbol` | Full symbol detail: signature, source body, children |

### Code Navigation
| Tool | Description |
|------|-------------|
| `find_references` | Inbound references — "who calls/uses this?" |
| `get_dependencies` | Outbound references — "what does this call/use?" |
| `get_implementations` | Trait/interface implementations (bidirectional) |

### Search
| Tool | Description |
|------|-------------|
| `search_docs` | Full-text keyword search over documentation |
| `search_symbols` | Full-text search over symbol names, signatures, doc comments |
| `semantic_search` | Natural language search via embedding similarity |

### Maintenance
| Tool | Description |
|------|-------------|
| `diff_symbols` | Compare symbols between two snapshots (added/removed/modified) |
| `delete_snapshot` | Remove a snapshot and GC orphaned data |
| `gc` | Manually run garbage collection |

## Indexing Pipeline

When you trigger indexing, the server runs 8 steps:

1. **Resolve ref** — git ref → commit SHA (via gix)
2. **Deduplicate** — skip if this commit was already indexed
3. **Create snapshot** — record with status `"indexing"`
4. **Walk files** — traverse git tree, SHA-256 checksum each file, deduplicate unchanged files
5. **Parse** — markdown → doc chunks; source → tree-sitter symbols
6. **Full-text index** — commit to Tantivy
7. **Resolve references** — build the cross-symbol dependency graph
8. **Embed & finalize** — generate embeddings (batches of 96), mark snapshot `"ready"`

Excluded directories: `node_modules/`, `target/`, `.git/`, `vendor/`, `.next/`, `dist/`, `build/`, `__pycache__/`

## Testing

```sh
cargo test
```

Integration tests use [testcontainers](https://docs.rs/testcontainers) to spin up a real PostgreSQL instance — no manual database setup needed for tests.

## License

See [LICENSE](LICENSE) for details.
