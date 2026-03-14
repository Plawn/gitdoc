//! LLM prompt definitions and executor.
//!
//! Each LLM call site in the server uses a `LlmPrompt` constant that embeds
//! system prompt, temperature, max_tokens, response format, and Prometheus
//! operation label.  `LlmExecutor` is the single gateway to the LLM — it
//! prepends the system message, calls the client, and records metrics.
//!
//! Also contains the Prometheus metrics infrastructure (previously `metrics.rs`).

use std::sync::Arc;
use std::time::Instant;

use llm_ai::{
    AiClientError, CompletionMessage, CompletionResponse, OpenAiCompatibleClient, ResponseFormat,
    Role,
};
use prometheus::{exponential_buckets, histogram_opts, opts, HistogramVec, IntCounterVec};

use crate::error::GitdocError;

// ============================================================================
// Prometheus metrics (kept from metrics.rs)
// ============================================================================

static LLM_METRICS: std::sync::OnceLock<LlmMetrics> = std::sync::OnceLock::new();

struct LlmMetrics {
    input_tokens_total: IntCounterVec,
    output_tokens_total: IntCounterVec,
    cached_tokens_total: IntCounterVec,
    request_duration_seconds: HistogramVec,
    requests_total: IntCounterVec,
}

/// Create LLM metrics and return boxed collectors for Prometheus registration.
pub fn llm_collectors() -> Vec<Box<dyn prometheus::core::Collector>> {
    let m = LLM_METRICS.get_or_init(|| {
        let input_tokens_total = IntCounterVec::new(
            opts!(
                "gitdoc_llm_input_tokens_total",
                "Total LLM input tokens consumed"
            ),
            &["operation"],
        )
        .expect("metric can be created");

        let output_tokens_total = IntCounterVec::new(
            opts!(
                "gitdoc_llm_output_tokens_total",
                "Total LLM output tokens produced"
            ),
            &["operation"],
        )
        .expect("metric can be created");

        let cached_tokens_total = IntCounterVec::new(
            opts!(
                "gitdoc_llm_cached_tokens_total",
                "Total LLM cached input tokens"
            ),
            &["operation"],
        )
        .expect("metric can be created");

        let request_duration_seconds = HistogramVec::new(
            histogram_opts!(
                "gitdoc_llm_request_duration_seconds",
                "LLM request duration in seconds",
                // 100ms to ~100s
                exponential_buckets(0.1, 2.0, 11).unwrap()
            ),
            &["operation", "status"],
        )
        .expect("metric can be created");

        let requests_total = IntCounterVec::new(
            opts!("gitdoc_llm_requests_total", "Total LLM requests"),
            &["operation", "status"],
        )
        .expect("metric can be created");

        LlmMetrics {
            input_tokens_total,
            output_tokens_total,
            cached_tokens_total,
            request_duration_seconds,
            requests_total,
        }
    });

    vec![
        Box::new(m.input_tokens_total.clone()),
        Box::new(m.output_tokens_total.clone()),
        Box::new(m.cached_tokens_total.clone()),
        Box::new(m.request_duration_seconds.clone()),
        Box::new(m.requests_total.clone()),
    ]
}

fn record_success(resp: &CompletionResponse, operation: &str, duration_secs: f64) {
    if let Some(m) = LLM_METRICS.get() {
        m.input_tokens_total
            .with_label_values(&[operation])
            .inc_by(resp.input_tokens.max(0) as u64);
        m.output_tokens_total
            .with_label_values(&[operation])
            .inc_by(resp.output_tokens.max(0) as u64);
        m.cached_tokens_total
            .with_label_values(&[operation])
            .inc_by(resp.cached_tokens.max(0) as u64);
        m.request_duration_seconds
            .with_label_values(&[operation, "success"])
            .observe(duration_secs);
        m.requests_total
            .with_label_values(&[operation, "success"])
            .inc();
    }
}

fn record_error(operation: &str, duration_secs: f64) {
    if let Some(m) = LLM_METRICS.get() {
        m.request_duration_seconds
            .with_label_values(&[operation, "error"])
            .observe(duration_secs);
        m.requests_total
            .with_label_values(&[operation, "error"])
            .inc();
    }
}

/// Tracked completion — private, only called inside `LlmExecutor::run`.
async fn tracked_complete(
    client: &OpenAiCompatibleClient,
    messages: &[CompletionMessage<'_>],
    temperature: Option<f64>,
    response_format: ResponseFormat,
    max_tokens: Option<i32>,
    operation: &str,
) -> Result<CompletionResponse, AiClientError> {
    let start = Instant::now();
    let result = client
        .complete(messages, temperature, response_format, max_tokens)
        .await;
    let duration = start.elapsed().as_secs_f64();

    match &result {
        Ok(resp) => record_success(resp, operation, duration),
        Err(_) => record_error(operation, duration),
    }

    result
}

// ============================================================================
// LlmPrompt — self-contained prompt definition
// ============================================================================

/// A self-contained LLM prompt definition embedding system prompt, settings,
/// and the Prometheus operation label.
pub struct LlmPrompt {
    pub system: &'static str,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
    pub format: ResponseFormat,
    pub operation: &'static str,
}

// ============================================================================
// Prompt constants
// ============================================================================

pub const PROMPT_SUMMARY_CRATE: LlmPrompt = LlmPrompt {
    system: "You are a technical documentation expert. Given the module structure and public API \
             of a Rust crate, produce a concise summary (3-8 paragraphs) that explains:\n\
             1. What the crate does (purpose)\n\
             2. How it's organized (key modules)\n\
             3. Main types and their roles\n\
             4. Typical usage patterns\n\
             Be precise and technical. Do not invent information not present in the data.",
    temperature: Some(0.3),
    max_tokens: Some(2000),
    format: ResponseFormat::Text,
    operation: "summary_crate",
};

pub const PROMPT_SUMMARY_MODULE: LlmPrompt = LlmPrompt {
    system: "You are a technical documentation expert. Given the public symbols of a Rust module, \
             produce a concise summary (2-4 paragraphs) that explains what this module provides, \
             its key types and functions, and how they relate to each other. Be precise.",
    temperature: Some(0.3),
    max_tokens: Some(1500),
    format: ResponseFormat::Text,
    operation: "summary_module",
};

pub const PROMPT_SUMMARY_TYPE: LlmPrompt = LlmPrompt {
    system: "You are a technical documentation expert. Given a Rust type with its methods and trait \
             relationships, produce a concise summary (1-3 paragraphs) that explains what this type \
             represents, its key methods, and how it fits into the library. Be precise.",
    temperature: Some(0.3),
    max_tokens: Some(1000),
    format: ResponseFormat::Text,
    operation: "summary_type",
};

pub const PROMPT_CHEATSHEET_GENERATE: LlmPrompt = LlmPrompt {
    system: "You are a technical documentation expert. Given a codebase's structure, produce a \
             structured cheatsheet that an AI agent can use to quickly understand the repo. \
             Use these sections:\n\
             - **Purpose**: What the project does (1-2 sentences)\n\
             - **Architecture**: High-level organization (key modules, layers)\n\
             - **Key Types**: Most important structs/enums/traits with one-line descriptions\n\
             - **Key Functions/Entry Points**: Main functions and how to use them\n\
             - **Patterns & Conventions**: Naming conventions, error handling, common patterns\n\
             - **Gotchas**: Non-obvious behaviors, footguns, important constraints\n\n\
             Be precise, concise, and factual. Do not invent information not present in the data.\n\n\
             After the cheatsheet, output a line containing only `---SUMMARY---`, \
             then a 1-2 sentence description of what was generated (for the patch log).",
    temperature: Some(0.3),
    max_tokens: Some(3000),
    format: ResponseFormat::Text,
    operation: "cheatsheet_generate",
};

pub const PROMPT_CHEATSHEET_UPDATE: LlmPrompt = LlmPrompt {
    system: "You are updating an existing repo cheatsheet with new insights learned from a \
             conversation. Merge the learnings into the existing cheatsheet:\n\
             - Add new information to the appropriate sections\n\
             - Correct any information that the learnings contradict\n\
             - Do NOT remove existing correct information\n\
             - Keep the same section structure (Purpose, Architecture, Key Types, \
               Key Functions/Entry Points, Patterns & Conventions, Gotchas)\n\
             - If learnings don't add anything new, return the cheatsheet unchanged\n\n\
             Output the full updated cheatsheet, then a line containing only `---SUMMARY---`, \
             then a 1-2 sentence description of what changed (or 'No changes' if nothing was updated).",
    temperature: Some(0.3),
    max_tokens: Some(3000),
    format: ResponseFormat::Text,
    operation: "cheatsheet_update",
};

pub const PROMPT_CHEATSHEET_LEARNINGS: LlmPrompt = LlmPrompt {
    system: "You are an expert at extracting technical knowledge. Given a conversation about a \
             codebase, extract the key technical insights as bullet points. Focus on:\n\
             - Architectural patterns discovered\n\
             - Important types/functions and their roles\n\
             - Non-obvious behaviors or gotchas\n\
             - Conventions and patterns\n\
             - Corrections to initial assumptions\n\n\
             Only include facts that were confirmed in the conversation. \
             If the conversation was superficial with no real insights, respond with 'No significant learnings.'",
    temperature: Some(0.2),
    max_tokens: Some(1500),
    format: ResponseFormat::Text,
    operation: "cheatsheet_learnings",
};

pub const PROMPT_CONVERSE: LlmPrompt = LlmPrompt {
    system: "You are a code intelligence assistant embedded in a codebase exploration tool. \
             You answer questions about a codebase using the provided code context (symbols, docs, signatures). \
             Be precise and reference specific types, functions, and modules. \
             When showing code from the provided context, always cite the source file path (e.g. `src/foo.rs`). \
             If you generate example code that is NOT from the context, mark it clearly as `[generated example]`. \
             Prefer quoting verbatim from the provided context over paraphrasing or rewriting code. \
             If you cannot provide the exact source code for a symbol the user is asking about, \
             append: \"Tip: use `set_mode(\\\"granular\\\")` then `get_symbol` for the exact source code.\" \
             If the context is insufficient, say so. \
             Keep answers concise but thorough.",
    temperature: Some(0.3),
    max_tokens: Some(3000),
    format: ResponseFormat::Text,
    operation: "converse",
};

pub const PROMPT_EXPLAIN: LlmPrompt = LlmPrompt {
    system: "You are a code intelligence assistant. Given relevant documentation and code symbols \
             from a codebase, answer the user's question clearly and concisely. Reference specific \
             types, functions, and modules. If the context is insufficient, say so.",
    temperature: Some(0.3),
    max_tokens: Some(2000),
    format: ResponseFormat::Text,
    operation: "explain",
};

pub const PROMPT_CONDENSATION: LlmPrompt = LlmPrompt {
    system: "You are a summarization assistant. Condense the following conversation about a codebase \
             into a concise summary (~300 words). Preserve key technical facts, decisions, and \
             conclusions. The summary will be used as context for future questions in this conversation.",
    temperature: Some(0.2),
    max_tokens: Some(1000),
    format: ResponseFormat::Text,
    operation: "condensation",
};

pub const PROMPT_CONDENSATION_MERGE: LlmPrompt = LlmPrompt {
    system: "You are a summarization assistant. You are given an existing summary of earlier conversation \
             and new turns that followed. Produce a single merged summary (~300 words) that incorporates \
             all key technical facts, decisions, and conclusions from both the existing summary and the \
             new turns. The merged summary replaces the old one entirely.",
    temperature: Some(0.2),
    max_tokens: Some(1000),
    format: ResponseFormat::Text,
    operation: "condensation",
};

pub const PROMPT_DIFF_SUMMARIZE: LlmPrompt = LlmPrompt {
    system: "You are a technical changelog writer. Given a structured diff of symbols \
             (functions, types, modules) between two snapshots of a codebase, produce a \
             concise, human-readable changelog in markdown. Group changes by theme/area \
             when possible. Focus on what changed and why it matters, not implementation \
             details. Use bullet points. Keep it under 500 words.",
    temperature: Some(0.3),
    max_tokens: Some(2000),
    format: ResponseFormat::Text,
    operation: "diff_summarize",
};

pub const PROMPT_ARCHITECT_ADVISE: LlmPrompt = LlmPrompt {
    system: "You are an expert software architect advisor. Given the user's question and relevant \
             library profiles, stack rules, project profiles, architecture decisions, and patterns \
             from the knowledge base, provide a clear, actionable recommendation. Reference specific \
             libraries and rules when applicable. Be concise but thorough.",
    temperature: Some(0.3),
    max_tokens: Some(2000),
    format: ResponseFormat::Text,
    operation: "architect_advise",
};

pub const PROMPT_ARCHITECT_PROFILE: LlmPrompt = LlmPrompt {
    system: r#"You are an expert software analyst. Given context about a library (its cheatsheet, public API symbols, and module tree), generate a structured profile with these sections:

## What it is
One-sentence description.

## Primary use cases
Bullet list of what this library is best used for.

## Key APIs
The most important types, functions, and traits a user needs to know.

## Architecture style
How the library is organized (e.g. builder pattern, actor model, middleware stack, etc.).

## Strengths
What this library does well compared to alternatives.

## Limitations
Known limitations, missing features, or areas where alternatives might be better.

## Ecosystem fit
What other libraries it pairs well with, and where it sits in the ecosystem.

## Gotchas
Non-obvious behaviors, common mistakes, or surprising defaults.

## Version notes
Important changes or migration notes for the current version.

Be concise and factual. Focus on information that helps an AI coding agent make informed technology choices."#,
    temperature: Some(0.3),
    max_tokens: Some(3000),
    format: ResponseFormat::Text,
    operation: "architect_profile",
};

pub const PROMPT_ARCHITECT_COMPARE: LlmPrompt = LlmPrompt {
    system: "You are an expert software architect. Compare the given libraries and produce a structured comparison. For each library, provide:\n\n\
             1. **Fit Score** (1-10): How well it fits the stated criteria\n\
             2. **Pros**: Key advantages\n\
             3. **Cons**: Key disadvantages\n\
             4. **Differentiator**: What makes it unique\n\n\
             Then provide a **Recommendation** section with your pick and reasoning.\n\n\
             Be concise, factual, and actionable.",
    temperature: Some(0.3),
    max_tokens: Some(3000),
    format: ResponseFormat::Text,
    operation: "architect_compare",
};

// ============================================================================
// LlmExecutor
// ============================================================================

/// Thin wrapper over `&OpenAiCompatibleClient` that handles system-message
/// injection and Prometheus tracking.
pub struct LlmExecutor<'a>(&'a OpenAiCompatibleClient);

impl<'a> LlmExecutor<'a> {
    pub fn new(client: &'a OpenAiCompatibleClient) -> Self {
        Self(client)
    }

    /// Prepends the system message from `prompt`, calls `tracked_complete`,
    /// and returns the raw `CompletionResponse`.
    pub async fn run(
        &self,
        prompt: &LlmPrompt,
        user_messages: &[CompletionMessage<'_>],
    ) -> Result<CompletionResponse, AiClientError> {
        let mut messages = Vec::with_capacity(1 + user_messages.len());
        messages.push(CompletionMessage::new(Role::System, prompt.system));
        messages.extend_from_slice(user_messages);

        tracked_complete(
            self.0,
            &messages,
            prompt.temperature,
            prompt.format,
            prompt.max_tokens,
            prompt.operation,
        )
        .await
    }

    /// Like `run()` but maps `AiClientError` to `anyhow::Error`.
    pub async fn run_anyhow(
        &self,
        prompt: &LlmPrompt,
        user_messages: &[CompletionMessage<'_>],
    ) -> anyhow::Result<CompletionResponse> {
        self.run(prompt, user_messages)
            .await
            .map_err(|e| anyhow::anyhow!("LLM error: {e}"))
    }

    /// Extract from `Option<&Arc<OpenAiCompatibleClient>>`, returning
    /// `GitdocError::ServiceUnavailable` on `None`.
    pub fn try_from_state(
        client: Option<&'a Arc<OpenAiCompatibleClient>>,
    ) -> Result<Self, GitdocError> {
        client
            .map(|c| Self::new(c.as_ref()))
            .ok_or_else(|| GitdocError::ServiceUnavailable("no LLM provider configured".into()))
    }
}
