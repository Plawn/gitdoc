# PLAN.md — GitDoc: Dynamic Mode Switching & Ask Improvements

## Context

This plan addresses the gaps identified during agent usage of gitdoc in simple mode.
All code references verified against the indexed codebase (snapshot master).

## Current State

### How modes work today
- Mode is **static**, set at launch via `GITDOC_MCP_MODE` env var
- `ModeFilter` in `gitdoc-mcp/src/mode_filter.rs` implements `CapabilityFilter`
  - Has a single field: `is_granular: bool`
  - `filter_tools()` returns all tools if granular, or only `SIMPLE_TOOLS` if simple
- `SIMPLE_TOOLS` constant lists the 9 simple-mode tool names
- `check_granular()` in `server.rs` is a call-time guard on `#[tool(guard = "check_granular")]` tools
- `filter_tools()` is called on **every** `tools/list` request (not once at startup)
- `CapabilityRegistry::notify_tools_changed()` is **public** in mcp-framework — no framework change needed

### How McpApp is assembled (main.rs)
```
Config → is_granular bool
       → ModeFilter { is_granular }
       → McpApp { capability_filter: Some(Arc::new(mode_filter)), ... }
       → mcp_framework::run(app)
```
The `McpApp` uses the real mcp-framework struct with `server_factory`, `capability_registry`,
`capability_filter`, `session_store` fields.

### How ask works
- `ask` tool in gitdoc-mcp calls `POST /snapshots/:id/converse` on gitdoc-server
- gitdoc-server does semantic search → context assembly → LLM synthesis
- Conversation state maintained server-side per snapshot
- Cheatsheet auto-generated on first ask, auto-injected into prompts

---

## Phase 1: Dynamic set_mode tool

### Goal
Agent can switch simple ↔ granular mid-session without restarting gitdoc-mcp.

### Implementation

#### 1.1 Make ModeFilter mutable

File: `gitdoc-mcp/src/mode_filter.rs`

Change `is_granular: bool` to use interior mutability:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ModeFilter {
    is_granular: AtomicBool,
}

impl ModeFilter {
    pub fn new(is_granular: bool) -> Self {
        Self { is_granular: AtomicBool::new(is_granular) }
    }

    pub fn set_granular(&self, value: bool) {
        self.is_granular.store(value, Ordering::SeqCst);
    }

    pub fn is_granular(&self) -> bool {
        self.is_granular.load(Ordering::SeqCst)
    }
}
```

Update `filter_tools` to use `self.is_granular()` instead of `self.is_granular`.

Update `SIMPLE_TOOLS` to include `"set_mode"`:
```rust
pub const SIMPLE_TOOLS: &[&str] = &[
    "ping",
    "list_repos",
    "register_repo",
    "index_repo",
    "get_repo_overview",
    "ask",
    "conversation_reset",
    "architect_advise",
    "compare_libs",
    "set_mode",  // NEW — always visible
];
```

#### 1.2 Add set_mode tool

File: `gitdoc-mcp/src/tools/set_mode.rs` (or wherever tools are defined)

Declare the tool on the Server struct:
```rust
#[tool]
pub set_mode: SetModeTool,
```

No `guard = "check_granular"` — this tool must be accessible in both modes.

Handler:
```rust
async fn set_mode(&self, input: SetModeInput) -> Result<CallToolResult, McpError> {
    let new_mode = match input.mode.as_str() {
        "simple" => false,
        "granular" => true,
        _ => return Err(McpError::InvalidParams("mode must be 'simple' or 'granular'".into())),
    };

    // 1. Update the filter
    self.mode_filter.set_granular(new_mode);

    // 2. Update the call-time guard state
    self.is_granular.store(new_mode, Ordering::SeqCst);

    // 3. Notify client to re-fetch tools/list
    self.capability_registry.notify_tools_changed().await;

    // 4. Return descriptive message
    let msg = if new_mode {
        "Switched to granular mode. You now have access to 52 tools including:\n\
         - get_symbol, list_symbols: inspect code symbols with full source\n\
         - read_doc, list_docs: read documentation files\n\
         - find_references, get_dependencies: navigate the call graph\n\
         - search_symbols, semantic_search: targeted search\n\
         - get_module_tree, get_public_api: high-level views\n\
         - get_type_context, get_examples: deep type exploration\n\
         - Cheatsheet, Architect KB management tools"
    } else {
        "Switched to simple mode. Available tools: ping, list_repos, register_repo, \
         index_repo, get_repo_overview, ask, conversation_reset, architect_advise, \
         compare_libs, set_mode."
    };
    Ok(CallToolResult::success(vec![Content::text(msg)]))
}
```

Input schema:
```json
{
    "type": "object",
    "properties": {
        "mode": {
            "type": "string",
            "enum": ["simple", "granular"],
            "description": "Tool mode. 'simple' (9 tools, conversational) or 'granular' (52 tools, full control). Use 'granular' when you need exact source code, symbol definitions, or fine-grained navigation."
        }
    },
    "required": ["mode"]
}
```

#### 1.3 Wire references in Server struct

The Server struct needs shared access to `ModeFilter` and `CapabilityRegistry`:
- `ModeFilter` is currently created in main.rs and moved into the `McpApp`
- For `set_mode` to mutate it, the Server needs an `Arc<ModeFilter>` reference
- Similarly, Server needs `Arc<CapabilityRegistry>` to call `notify_tools_changed()`

Changes in `main.rs`:
```rust
let mode_filter = Arc::new(ModeFilter::new(is_granular));
let registry = Arc::new(CapabilityRegistry::default());

// Register all tools in the registry...

// Pass Arc clones to the server factory
let mf = mode_filter.clone();
let reg = registry.clone();
let server_factory = move |token_store, session_store| {
    GitdocMcpServer::new(mf.clone(), reg.clone(), /* ... */)
};

run(McpApp {
    capability_filter: Some(mode_filter.clone() as Arc<dyn CapabilityFilter>),
    capability_registry: Some(registry),
    // ...
}).await
```

**Key consideration**: Check if `McpApp` takes `Option<CapabilityRegistry>` or `Option<Arc<CapabilityRegistry>>`. If it takes ownership, you may need to restructure so both the McpApp and the server_factory closure share the same Arc. Look at the exact `McpApp` fields and `run()` signature.

#### 1.4 Update check_granular

The `check_granular` guard in `server.rs` currently reads `self.is_granular` (a bool).
Change it to read from the shared `ModeFilter`:

```rust
fn check_granular(&self) -> Result<(), McpError> {
    if !self.mode_filter.is_granular() {
        Err(McpError::ModeRestricted)
    } else {
        Ok(())
    }
}
```

#### 1.5 Update instructions

In `instructions.rs`, update `SIMPLE_INSTRUCTIONS` to mention `set_mode`:
```
10. `set_mode`: Switch to 'granular' mode for direct access to source code, symbols,
    and references. Switch back to 'simple' for conversational exploration.
```

#### 1.6 Tests

- Test: start in simple → `tools/list` returns 10 tools (9 + set_mode)
- Test: call `set_mode("granular")` → `tools/list` returns 52+ tools
- Test: call `set_mode("simple")` → back to 10 tools
- Test: call a granular tool in simple mode → `McpError::ModeRestricted`
- Test: call `set_mode("invalid")` → `InvalidParams` error

---

## Phase 2: Improve ask source reliability

### Goal
Agent can distinguish real code (from the index) vs. LLM-generated examples.

### Implementation

#### 2.1 Server-side: tag sources in converse response

File: `gitdoc-server/src/api/converse.rs` (or wherever the LLM prompt is assembled)

When building the LLM prompt context from semantic search results, each code chunk
should include its origin metadata:

```
[source: gitdoc-mcp/src/mode_filter.rs, lines 12-28, symbol: ModeFilter]
```rust
pub struct ModeFilter {
    is_granular: bool,
}
```

Modify the system prompt for the converse LLM to instruct it:
- Always cite the source file path when showing code from the context
- Mark any code it generates itself as `[generated example]`
- Prefer quoting verbatim from the provided context over paraphrasing

#### 2.2 Add detail_level parameter to ask

File: `gitdoc-mcp/src/tools/ask.rs` (or wherever the ask tool input is defined)

Add optional parameter:
```json
{
    "detail_level": {
        "type": "string",
        "enum": ["brief", "detailed", "with_source"],
        "description": "Level of detail. 'brief' for concise answers, 'detailed' for thorough analysis, 'with_source' to include verbatim source code from the index."
    }
}
```

Pass this to the `/converse` endpoint. Server-side, when `with_source`:
- Include full source snippets inline in the response
- Attach a `sources` array to the response JSON with file paths, line ranges, and raw content

#### 2.3 Hint toward granular mode

When the LLM detects it's answering about specific code but can't provide exact source
(e.g., semantic search returned docs but not the actual symbol), append:

```
---
💡 For exact source code, use `set_mode("granular")` then `get_symbol` or `read_doc`.
```

This can be done in the system prompt:
```
If you cannot provide the exact source code for a symbol or file the user is asking about,
append a note suggesting they use set_mode("granular") for direct access.
```

---

## Phase 3: Promote key granular tools to simple mode

### Goal
Some granular tools are useful enough to be always available without requiring mode switch.

### Candidates for promotion

Based on real usage patterns during our session:

| Tool | Why promote | Risk |
|------|-------------|------|
| `get_cheatsheet` | Agents should see the persistent knowledge without switching | Low — read-only |
| `list_cheatsheet_patches` | See knowledge evolution | Low — read-only |
| `get_repo_overview` | Already in simple | N/A |
| `explain` | Semantic search + type context, richer than `ask` for specific questions | Medium — might confuse with `ask` |

### Implementation

Add promoted tool names to `SIMPLE_TOOLS` in `mode_filter.rs`.
Remove `guard = "check_granular"` from their `#[tool]` declarations.
Update `SIMPLE_INSTRUCTIONS` to document them.

**Decision needed**: Which tools to promote. Start conservatively with `get_cheatsheet` only.

---

## Phase 4: Improve agent onboarding

### Goal
First-time agents get oriented quickly.

### Implementation

#### 4.1 Better errors for missing snapshots

When any tool that requires a snapshot is called without one:
```
Error: No snapshot found for repo 'foo'. 
You must call index_repo(repo_id: "foo") first to create a snapshot.
```

Check: is this already the case? Verify error messages in snapshot_resolver.rs.

#### 4.2 Auto-suggest workflow in tool descriptions

Update the `register_repo` tool description to say:
```
After registering, call index_repo to create a snapshot before querying.
```

Update `ask` description:
```
Requires a snapshot. If you haven't indexed yet, call register_repo then index_repo first.
```

These are already somewhat present (per README tool descriptions) — verify they match
the actual tool `description` fields in the code.

#### 4.3 Token budget visibility

Add a field to the `ask` response that includes:
```json
{
    "answer": "...",
    "context_tokens_used": 8500,
    "context_tokens_budget": 12000,
    "hint": "Context is 71% full. Consider conversation_reset if switching topics."
}
```

This requires a server-side change in the `/converse` response.

---

## Execution Order

```
Phase 1 (set_mode) → can be done entirely in gitdoc-mcp
    1.1 ModeFilter AtomicBool .............. 15 min
    1.2 set_mode tool handler .............. 30 min
    1.3 Wire Arc<ModeFilter> + Arc<CapabilityRegistry> in main.rs .. 30 min
    1.4 Update check_granular .............. 5 min
    1.5 Update instructions ................ 5 min
    1.6 Tests .............................. 30 min

Phase 2 (ask reliability) → mostly gitdoc-server
    2.1 Tag sources in LLM prompt .......... 1-2 hr (depends on prompt complexity)
    2.2 detail_level parameter ............. 30 min
    2.3 Granular mode hints ................ 15 min

Phase 3 (promote tools) → gitdoc-mcp only
    Small change, do after validating Phase 1 works .. 15 min

Phase 4 (onboarding) → both crates
    4.1 Error messages ..................... 30 min
    4.2 Tool descriptions .................. 15 min
    4.3 Token budget visibility ............ 1 hr (server change)
```

## Files to Touch

### gitdoc-mcp
- `src/mode_filter.rs` — AtomicBool, add "set_mode" to SIMPLE_TOOLS
- `src/server.rs` — add set_mode tool, update check_granular to read from shared ModeFilter
- `src/main.rs` — Arc<ModeFilter>, Arc<CapabilityRegistry>, wire to server_factory
- `src/instructions.rs` — add set_mode to SIMPLE_INSTRUCTIONS
- `src/tools/` — new set_mode tool file (if tools are in separate files)
- `tests/` — mode switching integration tests

### gitdoc-server
- `src/api/converse.rs` — source tagging, detail_level support, token budget in response
- LLM system prompt (wherever it's defined) — source citation instructions
- `src/api/` — error message improvements for missing snapshots

### mcp-framework
- **No changes needed** — `notify_tools_changed()` is already public