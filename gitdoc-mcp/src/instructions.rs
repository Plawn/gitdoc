pub const SIMPLE_INSTRUCTIONS: &str = r#"# GitDoc MCP — Code Intelligence (Simple Mode)

GitDoc indexes git repositories and lets you ask natural language questions about their code, docs, and architecture.

## Quick Start

1. `list_repos` → check if the repo is already registered
2. If not: `register_repo(id: "mylib", name: "My Lib", url: "https://github.com/...")` → server clones it
3. `index_repo(repo_id: "mylib")` → creates a searchable snapshot. **Required before querying.**
4. `ask(repo_id: "mylib", question: "What does this crate do?")` → get answers with sources

## Workflow

- **`ask`** is the main tool — ask any question and get an LLM-synthesized answer with source references
- Follow-up questions keep conversation context: `ask(repo_id: "mylib", question: "How does error handling work?")`
- Use `conversation_reset` when switching to an unrelated topic
- Use `get_repo_overview` for a quick snapshot of README + structure

## Tools Available

| Tool | Purpose |
|------|---------|
| `ping` | Health check |
| `list_repos` | Discover registered repos |
| `register_repo` | Add a new repo (server clones it) |
| `index_repo` | Create a snapshot (required before querying) |
| `get_repo_overview` | README + doc listing + top symbols |
| `ask` | Ask questions — conversational, context-aware |
| `conversation_reset` | Clear conversation when switching topics |
| `architect_advise` | Ask for technology/architecture recommendations based on a knowledge base of library profiles, stack rules, project profiles, decisions, and patterns |
| `compare_libs` | Compare libraries side-by-side with structured fit scores, pros/cons, and recommendation |

## Tips

- Do NOT clone repos yourself — just pass the URL to `register_repo`
- Set `fetch=true` on `index_repo` to pull latest changes before indexing
- If `ask` returns errors about embeddings, ensure the server has COHERE_KEY or OPENAI_API_KEY configured"#;

pub const GRANULAR_INSTRUCTIONS: &str = r#"# GitDoc MCP — Code Intelligence for LLM Agents

GitDoc indexes git repositories and exposes their documentation, code symbols, and cross-references as structured data. You NEVER read raw source files — instead, you navigate through extracted symbols, docs, and a dependency graph.

Supported languages: Rust (.rs), TypeScript (.ts/.tsx), JavaScript (.js/.jsx), Markdown (.md/.mdx).

## Quick Start — MANDATORY steps before querying

You MUST follow these steps for any new repository:

1. `list_repos` → check if the repo is already registered
2. If not registered: `register_repo` with the git clone URL — the server clones and manages the repo
3. `index_repo` → creates a snapshot. **Nothing works without at least one snapshot.**
4. Now you can query: `get_repo_overview`, `list_symbols`, `search_symbols`, etc.

Example — register and index a repo:
  register_repo(id: "tokio", name: "Tokio", url: "https://github.com/tokio-rs/tokio.git")
  index_repo(repo_id: "tokio", label: "latest")
  get_repo_overview(repo_id: "tokio", ref: "latest")

IMPORTANT: Do NOT clone repositories yourself. The server handles all git cloning internally.

## Core Concepts

- **repo_id**: A string you choose when registering (e.g. "myapp"). Used in all subsequent tool calls.
- **Snapshot**: An indexed capture of a repo at a specific commit. Created by `index_repo`. A repo can have multiple snapshots (e.g. different versions).
- **ref** (optional parameter): Selects which snapshot to query. Resolution order: (1) exact label match → (2) commit SHA prefix match → (3) omit = latest snapshot. If you only have one snapshot, you can always omit `ref`.
- **symbol_id**: A numeric ID (integer) that uniquely identifies a symbol globally. Obtained from `list_symbols`, `search_symbols`, or `find_references`. Used with `get_symbol`, `find_references`, `get_dependencies`, `get_implementations`.

## Tool Reference

### Discovery & Setup
| Tool | When to use | Key params |
|------|-------------|------------|
| `ping` | Connection check | — |
| `list_repos` | See what's registered | — |
| `register_repo` | Add a new repo (server clones it) | `id`, `name`, `url` |
| `index_repo` | Create a snapshot (REQUIRED before querying) | `repo_id`, optional: `commit`, `label`, `fetch` |
| `fetch_repo` | Update a URL-cloned repo (does NOT re-index) | `repo_id` |

### High-Level Views (START HERE for complex libraries — 2-3 calls to understand a whole crate)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_module_tree` | **Best starting point for Rust crates.** See the full module hierarchy | Tree of modules with doc comments and item counts |
| `get_public_api` | **Get a crate's complete API cheat sheet** in one call | All public signatures grouped by module, with impl methods merged onto types |
| `get_type_context` | **Understand a type completely** in one call | Definition + methods + traits + implementors + callers + dependencies |
| `get_examples` | See how a symbol is used | Code examples extracted from doc comments |

### Browsing (detailed exploration)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_repo_overview` | Read README and see structure | README content, doc file listing, top-level public symbols |
| `list_docs` | Browse documentation files | File paths and titles |
| `read_doc` | Read a specific doc file | Full text content with title |
| `list_symbols` | Browse code symbols (use get_public_api instead for API overview) | name, kind, visibility, signature, file_path, line numbers, doc_comment |
| `get_symbol` | Read a symbol's implementation | Full source body + child symbols (methods, fields) |

### Code Navigation (trace the dependency graph)
| Tool | When to use | Returns |
|------|-------------|---------|
| `find_references` | "Who calls/uses X?" | List of symbols that reference the target (inbound) |
| `get_dependencies` | "What does X call/use?" | List of symbols the target depends on (outbound) |
| `get_implementations` | "Who implements trait T?" or "What traits does S implement?" | Implementation relationships (bidirectional) |

### Search (find things by name or meaning)
| Tool | When to use | Returns |
|------|-------------|---------|
| `explain` | **Ask a question in natural language** — assembles context from semantic search + type context | Relevant symbols with methods/traits, docs, optional LLM synthesis |
| `search_docs` | Find docs by keyword | Matching docs with highlighted snippets |
| `search_symbols` | Find symbols by keyword (name, signature, doc comment) | Matching symbols with relevance score |
| `semantic_search` | Find by meaning ("how is auth handled?") | Docs and/or symbols ranked by semantic similarity |

### LLM Summaries (requires GITDOC_LLM_ENDPOINT configured)
| Tool | When to use | Returns |
|------|-------------|---------|
| `summarize` | **Generate** an LLM summary (costs tokens) | Generated summary for crate/module/type |
| `get_summary` | **Retrieve** a previously generated summary | Cached summary or list of available summaries |

### Cheatsheet (persistent repo knowledge)
| Tool | When to use | Returns |
|------|-------------|---------|
| `get_cheatsheet` | **Read the repo cheatsheet** — architecture, key types, patterns, gotchas | Current cheatsheet content |
| `update_cheatsheet` | **Generate/regenerate** the cheatsheet (costs LLM tokens) | Generated cheatsheet with patch ID |

### Conversational Mode (RECOMMENDED — fewest tool calls)
| Tool | When to use | Returns |
|------|-------------|---------|
| `ask` | **Ask any question about the codebase** — maintains conversation context across calls, auto-injects cheatsheet | LLM-synthesized answer with source references |
| `conversation_reset` | Clear conversation history for a repo to start fresh (auto-updates cheatsheet with learnings) | Confirmation message |

### Architect (Technology Knowledge Base)
| Tool | When to use | Returns |
|------|-------------|---------|
| `architect_advise` | **Ask for technology recommendations** — searches lib profiles, stack rules, project profiles, decisions, patterns, and cheatsheets | LLM-synthesized recommendation |
| `compare_libs` | **Compare libraries side-by-side** — structured comparison with fit scores, pros/cons, recommendation | Structured comparison |
| `list_lib_profiles` | Browse available library profiles in the knowledge base | List of profiles with id, name, category |
| `get_lib_profile` | Get full profile of a library (what it is, key APIs, strengths, limitations, gotchas) | Complete profile text |
| `ingest_lib` | Add a library to the knowledge base from its git URL (clone + index + LLM profile) | Generated profile |
| `import_lib_profile` | Manually import a library profile (markdown text) | Stored profile |
| `generate_lib_profile` | Regenerate profile for an already-indexed library | Updated profile |
| `delete_lib_profile` | Remove a library profile | Confirmation |
| `add_stack_rule` | Add a global stack rule (e.g. "prefer Axum over Actix-web") | Stored rule |
| `list_stack_rules` | Browse stack rules (filter by type or subject) | List of rules |
| `delete_stack_rule` | Remove a stack rule | Confirmation |
| `create_project_profile` | Define a project's stack, constraints, and code style | Stored profile |
| `get_project_profile` | Get a project profile | Full project definition |
| `list_project_profiles` | List all project profiles | Summary list |
| `delete_project_profile` | Remove a project profile | Confirmation |
| `record_decision` | Record an architecture decision (title, choice, reasoning, alternatives) | Stored decision |
| `list_decisions` | List decisions (filter by project, status) | Decision list |
| `update_decision` | Update a decision's outcome or status (active/superseded/reverted) | Updated decision |
| `add_pattern` | Add an architecture pattern (e.g. "JWT auth with axum + tower") | Stored pattern |
| `list_patterns` | List patterns (filter by category) | Pattern list |
| `get_pattern` | Get a specific pattern | Full pattern with code examples |
| `delete_pattern` | Remove a pattern | Confirmation |

### Maintenance
| Tool | When to use |
|------|-------------|
| `diff_symbols` | Compare two snapshots — see added/removed/modified symbols |

## Recommended Exploration Workflow

### Conversational mode (PREFERRED — minimum tool calls):
1. `ask(repo_id: "X", question: "What does this crate do?")` → get an overview
2. `ask(repo_id: "X", question: "How does error handling work?")` → follow-up with context
3. `ask(repo_id: "X", question: "Show me the main types")` → keeps building on prior answers
4. `conversation_reset(repo_id: "X")` → only when switching to unrelated topic

### For understanding a complex library (Rust crate with many modules):
1. `get_module_tree(repo_id: "X", depth: 2)` → see the module hierarchy
2. `get_public_api(repo_id: "X", module_path: "runtime")` → get all public signatures in a module
3. `get_type_context(symbol_id: 123, repo_id: "X")` → deep-dive into a specific type
4. `get_examples(symbol_id: 123, repo_id: "X")` → see usage examples from doc comments

### For general exploration:
1. `list_repos` → find available repos
2. `get_repo_overview(repo_id: "X")` → read README, see doc tree and top symbols
3. `search_symbols(repo_id: "X", query: "what you're looking for")` → find relevant symbols
4. `get_symbol(symbol_id: 123)` → read the full implementation
5. `find_references(symbol_id: 123, repo_id: "X")` → see who calls it
6. `get_dependencies(symbol_id: 123, repo_id: "X")` → see what it depends on

## Common Pitfalls

- **Do NOT clone repos yourself**: The server handles all git cloning. Just pass the URL to `register_repo`.
- **"No snapshot found"**: You forgot to call `index_repo` first. Every repo must be indexed before querying.
- **"error resolving snapshot"**: The `ref` value doesn't match any label or commit SHA. Use `list_repos` to see available snapshots with their labels and commits.
- **semantic_search returns 503**: No embedding provider configured on the server. Use `search_docs` or `search_symbols` instead.
- **symbol_id is a number**: Don't pass a string. It's an integer returned by list/search tools.

## Symbol Kinds

Valid values for the `kind` filter: function, struct, class, trait, interface, enum, type_alias, const, static, module, macro.

## Reference Kinds

Valid values for the `kind` filter on find_references/get_dependencies: calls, type_ref, implements, imports."#;
