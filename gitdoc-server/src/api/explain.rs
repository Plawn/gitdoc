use r2e::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

use gitdoc_api_types::requests::ExplainQuery;
use gitdoc_api_types::responses::{ExplainResult, RelevantSymbol, MethodInfo, RelevantDoc};

use crate::AppState;
use crate::embeddings;
use crate::error::GitdocError;

#[derive(Controller)]
#[controller(path = "/snapshots", state = AppState)]
pub struct ExplainController {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[routes]
impl ExplainController {
    #[get("/{snapshot_id}/explain")]
    async fn explain(
        &self,
        Path(snapshot_id): Path<i64>,
        Query(q): Query<ExplainQuery>,
    ) -> Result<Json<ExplainResult>, GitdocError> {
        if q.q.is_empty() {
            return Err(GitdocError::BadRequest("q must be non-empty".into()));
        }

        let synthesize = q.synthesize.unwrap_or(false);
        let limit = q.limit.unwrap_or(10);

        let embedder = self.embedder.as_ref()
            .ok_or_else(|| GitdocError::ServiceUnavailable(
                "no embedding provider configured — explain requires embeddings".into(),
            ))?;

        let query_vec = embedder.embed_query(&q.q).await
            .map_err(|e| GitdocError::Internal(e))?;

        let file_ids = self.db.get_file_ids_for_snapshot(snapshot_id).await?;
        let query_pgvec = embeddings::to_pgvector(&query_vec);

        let search_results = self
            .db
            .search_embeddings_by_vector(&query_pgvec, &file_ids, "all", limit as i64)
            .await?;

        let docs = self.db.list_docs_for_snapshot(snapshot_id).await.unwrap_or_else(|e| {
            tracing::warn!(snapshot_id, error = %e, "failed to list docs for snapshot");
            Vec::new()
        });

        let mut relevant_symbols: Vec<RelevantSymbol> = Vec::new();
        let mut relevant_docs: Vec<RelevantDoc> = Vec::new();
        let mut seen_symbols: HashSet<i64> = HashSet::new();

        for r in &search_results {
            match r.source_type.as_str() {
                "symbol" => {
                    if seen_symbols.contains(&r.source_id) {
                        continue;
                    }
                    seen_symbols.insert(r.source_id);

                    if let Ok(Some(sym)) = self.db.get_symbol_by_id(r.source_id).await {
                        let methods = if matches!(sym.kind.as_str(), "struct" | "enum" | "trait" | "class" | "interface") {
                            let children = self.db.list_symbol_children(sym.id).await.unwrap_or_else(|e| {
                                tracing::warn!(symbol_id = sym.id, error = %e, "failed to list symbol children");
                                Vec::new()
                            });
                            children
                                .iter()
                                .filter(|c| c.kind == "function")
                                .map(|c| MethodInfo {
                                    name: c.name.clone(),
                                    signature: c.signature.clone(),
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };

                        let traits = if matches!(sym.kind.as_str(), "struct" | "enum" | "class") {
                            let impls = self.db.get_implementations(sym.id, snapshot_id).await.unwrap_or_else(|e| {
                                tracing::warn!(symbol_id = sym.id, snapshot_id, error = %e, "failed to get implementations");
                                Vec::new()
                            });
                            impls.iter()
                                .filter(|i| i.symbol.kind == "trait" || i.symbol.kind == "interface")
                                .map(|i| i.symbol.qualified_name.clone())
                                .collect()
                        } else {
                            Vec::new()
                        };

                        relevant_symbols.push(RelevantSymbol {
                            id: sym.id,
                            name: sym.name,
                            qualified_name: sym.qualified_name,
                            kind: sym.kind,
                            signature: sym.signature,
                            doc_comment: sym.doc_comment,
                            file_path: sym.file_path,
                            score: r.score,
                            methods,
                            traits,
                        });
                    }
                }
                "doc_chunk" => {
                    if let Some(doc) = docs.iter().find(|d| d.id == r.source_id) {
                        relevant_docs.push(RelevantDoc {
                            file_path: doc.file_path.clone(),
                            title: doc.title.clone(),
                            snippet: r.text.clone(),
                            score: r.score,
                        });
                    }
                }
                _ => {}
            }
        }

        let synthesis = if synthesize {
            if let Some(ref llm_client) = self.llm_client {
                Some(synthesize_answer(llm_client, &q.q, &relevant_symbols, &relevant_docs).await?)
            } else {
                Some("LLM synthesis unavailable — no LLM provider configured (set GITDOC_LLM_ENDPOINT)".into())
            }
        } else {
            None
        };

        let result = ExplainResult {
            query: q.q,
            relevant_symbols,
            relevant_docs,
            synthesis,
        };

        Ok(Json(result))
    }
}

async fn synthesize_answer(
    client: &llm_ai::OpenAiCompatibleClient,
    query: &str,
    symbols: &[RelevantSymbol],
    docs: &[RelevantDoc],
) -> Result<String, GitdocError> {
    use llm_ai::{CompletionMessage, ResponseFormat, Role};

    let mut context = String::new();

    if !docs.is_empty() {
        context.push_str("## Relevant documentation\n\n");
        for doc in docs.iter().take(5) {
            context.push_str(&format!("### {} ({})\n{}\n\n",
                doc.title.as_deref().unwrap_or("untitled"),
                doc.file_path,
                doc.snippet,
            ));
        }
    }

    if !symbols.is_empty() {
        context.push_str("## Relevant symbols\n\n");
        for sym in symbols.iter().take(10) {
            context.push_str(&format!("### {} ({}) — {}\n", sym.name, sym.kind, sym.file_path));
            context.push_str(&format!("Signature: {}\n", sym.signature));
            if let Some(ref doc) = sym.doc_comment {
                let first_lines: String = doc.lines().take(5).collect::<Vec<_>>().join("\n");
                context.push_str(&format!("Doc: {}\n", first_lines));
            }
            if !sym.methods.is_empty() {
                context.push_str("Methods:\n");
                for m in sym.methods.iter().take(10) {
                    context.push_str(&format!("  - {}: {}\n", m.name, m.signature));
                }
            }
            if !sym.traits.is_empty() {
                context.push_str(&format!("Implements: {}\n", sym.traits.join(", ")));
            }
            context.push('\n');
        }
    }

    let user_msg = format!("Question: {}\n\n{}", query, context);
    let messages = vec![
        CompletionMessage::new(
            Role::System,
            "You are a code intelligence assistant. Given relevant documentation and code symbols \
             from a codebase, answer the user's question clearly and concisely. Reference specific \
             types, functions, and modules. If the context is insufficient, say so.",
        ),
        CompletionMessage::new(
            Role::User,
            &user_msg,
        ),
    ];

    let resp = client
        .complete(&messages, Some(0.3), ResponseFormat::Text, Some(2000))
        .await
        .map_err(|e| GitdocError::Internal(anyhow::anyhow!("LLM error: {e}")))?;

    Ok(resp.content)
}
