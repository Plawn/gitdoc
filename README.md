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
- **Persistent repo cheatsheets** — LLM-generated architecture/types/patterns summaries that accumulate knowledge across sessions
- **Conversational mode** — multi-turn Q&A with context persistence; cheatsheet auto-injected into prompts
- **Two MCP modes** — simple (7 tools, conversational) for coding agents; granular (21 tools) for fine-grained control

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

By default the MCP server starts in **simple mode** (7 tools, conversational). Set `GITDOC_MCP_MODE=granular` for all 21 tools.

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

For granular mode (all tools):

```json
{
  "mcpServers": {
    "gitdoc": {
      "command": "cargo",
      "args": ["run", "--bin", "gitdoc-mcp"],
      "cwd": "/path/to/gitdoc",
      "env": {
        "GITDOC_SERVER_URL": "http://127.0.0.1:3000",
        "GITDOC_MCP_MODE": "granular"
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
| `GITDOC_LLM_ENDPOINT` | — | LLM API endpoint URL (see LLM setup below) |
| `GITDOC_LLM_KEY` | — | API key for the LLM endpoint |
| `GITDOC_LLM_MODEL` | — | Model/deployment name |
| `GITDOC_LLM_KIND` | `azure` | Engine kind: `azure`, `azure_inference`, or `ollama` |
| `GITDOC_MAX_PROMPT_TOKENS` | `12000` | Total token budget for conversation prompts |
| `GITDOC_CONDENSATION_THRESHOLD` | `6000` | Trigger history condensation after this many raw turn tokens |
| `RUST_LOG` | `info` | Tracing filter directive |

If neither embedding key is set, semantic search returns `503 Service Unavailable`. When both are set, Cohere takes precedence. If no LLM endpoint is set, summaries, cheatsheets, and conversational mode are unavailable.

### LLM Setup

The LLM powers summaries, cheatsheets (auto-generated on first `ask`), and conversational Q&A. GitDoc supports three engine kinds:

#### Azure OpenAI

```sh
GITDOC_LLM_KIND=azure
GITDOC_LLM_ENDPOINT=https://your-resource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-02-01
GITDOC_LLM_KEY=your-azure-api-key
GITDOC_LLM_MODEL=gpt-4o
```

#### Azure AI Inference (GitHub Models, Azure AI Studio)

```sh
GITDOC_LLM_KIND=azure_inference
GITDOC_LLM_ENDPOINT=https://models.inference.ai.azure.com/chat/completions
GITDOC_LLM_KEY=your-github-token-or-azure-key
GITDOC_LLM_MODEL=gpt-4o
```

#### Ollama (local)

```sh
GITDOC_LLM_KIND=ollama
GITDOC_LLM_ENDPOINT=http://localhost:11434/v1/chat/completions
GITDOC_LLM_MODEL=llama3.1
```

No API key needed for Ollama — just ensure the server is running (`ollama serve`).

#### TOML config alternative

Instead of environment variables, you can configure the LLM in `gitdoc.toml`:

```toml
[llm]
kind = "ollama"
endpoint = "http://localhost:11434/v1/chat/completions"
model = "llama3.1"
# key = "..." # optional, depending on provider
```

Environment variables take precedence over TOML values. See [`gitdoc.example.toml`](gitdoc.example.toml) for a complete example.

### gitdoc-mcp

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `GITDOC_SERVER_URL` | `http://127.0.0.1:3000` | URL of the gitdoc-server instance |
| `GITDOC_MCP_MODE` | `simple` | Tool mode: `simple` (7 conversational tools) or `granular` (all 21 tools) |

## API Reference

### Repos

```
POST /repos                         Create a repo  { id, name, url }
GET  /repos                         List all repos
GET  /repos/:repo_id                Get repo with its snapshots
POST /repos/:repo_id/index          Trigger indexing  { commit?, label?, fetch? }
POST /repos/:repo_id/fetch          Pull latest remote changes
DELETE /repos/:repo_id              Delete repo and clean up
```

### Cheatsheets

```
POST /repos/:repo_id/cheatsheet                  Generate/update cheatsheet  { snapshot_id, trigger? }
POST /repos/:repo_id/cheatsheet/stream            Generate with SSE progress  { snapshot_id, trigger? }
GET  /repos/:repo_id/cheatsheet                   Get current cheatsheet
GET  /repos/:repo_id/cheatsheet/patches            List patch history  ?limit=&offset=
GET  /repos/:repo_id/cheatsheet/patches/:patch_id  Get full patch (prev + new content)
```

The `/stream` endpoint returns Server-Sent Events with JSON payloads:
```
data: {"stage":"gathering","message":"Loading repo structure..."}
data: {"stage":"generating","message":"Calling LLM..."}
data: {"stage":"done","patch_id":5,"message":"Cheatsheet generated"}
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

### Conversations

```
POST   /snapshots/:id/converse                          Multi-turn Q&A  { q, conversation_id?, limit? }
DELETE /snapshots/:id/conversations/:conversation_id    Delete conversation (auto-updates cheatsheet)
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

The MCP server supports two modes, selected via `GITDOC_MCP_MODE`:

- **Simple mode** (default) — 7 tools centered around conversational `ask`. Best for coding agents like Claude Code that benefit from fewer, high-level tools.
- **Granular mode** — all 21 tools for fine-grained control over docs, symbols, references, and search.

Most tools accept `repo_id` + optional `ref`. If `ref` is omitted, the most recent snapshot is used.

### Simple Mode Tools (7)

| Tool | Description |
|------|-------------|
| `ping` | Health check — verify server is reachable |
| `list_repos` | List all registered repos with IDs, names, paths |
| `register_repo` | Register a repo by clone URL |
| `index_repo` | Create a snapshot at a commit (**required** before querying) |
| `get_repo_overview` | README + doc tree + top-level public symbols (start here) |
| `ask` | Multi-turn Q&A — maintains context across calls, auto-generates and injects cheatsheet |
| `conversation_reset` | Clear conversation history (auto-updates cheatsheet with learnings) |

### Additional Granular Mode Tools (+14)

#### Setup
| Tool | Description |
|------|-------------|
| `fetch_repo` | Pull latest changes for a URL-cloned repo |

#### Browsing
| Tool | Description |
|------|-------------|
| `list_docs` | List markdown/text files in a snapshot |
| `read_doc` | Read full content of a doc file |
| `list_symbols` | List code symbols with filters (kind, file_path, include_private) |
| `get_symbol` | Full symbol detail: signature, source body, children |

#### High-Level Views
| Tool | Description |
|------|-------------|
| `get_module_tree` | Hierarchical module tree with doc comments and item counts |
| `get_public_api` | Complete public API surface grouped by module |
| `get_type_context` | Full type detail: definition + methods + traits + implementors + callers |
| `get_examples` | Code examples extracted from doc comments |

#### Code Navigation
| Tool | Description |
|------|-------------|
| `find_references` | Inbound references — "who calls/uses this?" |
| `get_dependencies` | Outbound references — "what does this call/use?" |
| `get_implementations` | Trait/interface implementations (bidirectional) |

#### Search
| Tool | Description |
|------|-------------|
| `search_docs` | Full-text keyword search over documentation |
| `search_symbols` | Full-text search over symbol names, signatures, doc comments |
| `semantic_search` | Natural language search via embedding similarity |
| `explain` | Semantic search + type context assembly, optional LLM synthesis |

#### LLM Summaries
| Tool | Description |
|------|-------------|
| `summarize` | Generate an LLM summary for a crate/module/type |
| `get_summary` | Retrieve a previously generated summary |

#### Cheatsheet
| Tool | Description |
|------|-------------|
| `get_cheatsheet` | Read the persistent repo cheatsheet (architecture, key types, patterns, gotchas) |
| `update_cheatsheet` | Generate or regenerate the cheatsheet (LLM-powered) |

#### Maintenance
| Tool | Description |
|------|-------------|
| `diff_symbols` | Compare symbols between two snapshots (added/removed/modified) |

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
