use r2e::prelude::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::proto;
use crate::AppState;
use crate::llm_executor::{LlmExecutor, PROMPT_ARCHITECT_ADVISE};

#[derive(Controller)]
#[controller(state = AppState)]
pub struct ArchitectGrpcService {
    #[inject]
    db: Arc<crate::db::Database>,
    #[inject]
    embedder: Option<Arc<dyn crate::embeddings::EmbeddingProvider>>,
    #[inject]
    llm_client: Option<Arc<llm_ai::OpenAiCompatibleClient>>,
}

#[grpc_routes(proto::architect_service_server::ArchitectService)]
impl ArchitectGrpcService {
    // -----------------------------------------------------------------------
    // Libs
    // -----------------------------------------------------------------------

    async fn list_libs(
        &self,
        request: Request<proto::ListLibsRequest>,
    ) -> Result<Response<proto::ListLibsResponse>, Status> {
        let req = request.into_inner();
        let category = if req.category.is_empty() {
            None
        } else {
            Some(req.category.as_str())
        };
        let libs = self
            .db
            .list_lib_profiles(category)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListLibsResponse {
            libs: libs.into_iter().map(Into::into).collect(),
        }))
    }

    async fn create_lib(
        &self,
        request: Request<proto::CreateLibRequest>,
    ) -> Result<Response<proto::CreateLibResponse>, Status> {
        let req = request.into_inner();
        if req.id.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument("id and name are required"));
        }

        let category = if req.category.is_empty() { "general" } else { &req.category };
        let version_hint = if req.version_hint.is_empty() {
            "unknown"
        } else {
            &req.version_hint
        };
        let profile = if req.profile.is_empty() { "" } else { &req.profile };

        let embedding = if let Some(ref e) = self.embedder {
            let text = format!("{} {} {}", req.name, category, profile);
            e.embed_query(&text).await.ok()
        } else {
            None
        };
        let embedding_pgvec = embedding.map(|v| crate::embeddings::to_pgvector(&v));

        self.db
            .upsert_lib_profile(
                &req.id,
                &req.name,
                None,
                category,
                version_hint,
                profile,
                "manual",
                "n/a",
                embedding_pgvec,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let lib = self
            .db
            .get_lib_profile(&req.id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("lib profile vanished after creation"))?;

        Ok(Response::new(proto::CreateLibResponse {
            lib: Some(lib.into()),
        }))
    }

    async fn get_lib(
        &self,
        request: Request<proto::GetLibRequest>,
    ) -> Result<Response<proto::GetLibResponse>, Status> {
        let id = request.into_inner().id;
        let lib = self
            .db
            .get_lib_profile(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("lib profile not found"))?;
        Ok(Response::new(proto::GetLibResponse {
            lib: Some(lib.into()),
        }))
    }

    async fn delete_lib(
        &self,
        request: Request<proto::DeleteLibRequest>,
    ) -> Result<Response<proto::DeleteLibResponse>, Status> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .delete_lib_profile(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !deleted {
            return Err(Status::not_found("lib profile not found"));
        }
        Ok(Response::new(proto::DeleteLibResponse { deleted: true }))
    }

    async fn generate_lib_profile(
        &self,
        request: Request<proto::GenerateLibProfileRequest>,
    ) -> Result<Response<proto::GenerateLibProfileResponse>, Status> {
        let req = request.into_inner();
        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;

        // Look up the existing lib profile to get name, category, version_hint
        let existing = self
            .db
            .get_lib_profile(&req.lib_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| {
                Status::not_found("lib profile not found — create it first with CreateLib")
            })?;

        let snapshot_id = if req.snapshot_id == 0 {
            // Get latest snapshot for the repo
            let snapshots = self
                .db
                .list_snapshots(&req.repo_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            snapshots
                .last()
                .map(|s| s.id)
                .ok_or_else(|| Status::not_found("no snapshots found for repo"))?
        } else {
            req.snapshot_id
        };

        let lib = crate::architect::generate_lib_profile(
            llm_client,
            self.embedder.as_deref(),
            &self.db,
            &req.lib_id,
            &existing.name,
            &req.repo_id,
            snapshot_id,
            &existing.category,
            &existing.version_hint,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::GenerateLibProfileResponse {
            lib: Some(lib.into()),
        }))
    }

    // -----------------------------------------------------------------------
    // Rules
    // -----------------------------------------------------------------------

    async fn list_rules(
        &self,
        request: Request<proto::ListRulesRequest>,
    ) -> Result<Response<proto::ListRulesResponse>, Status> {
        let req = request.into_inner();
        let rule_type = if req.rule_type.is_empty() {
            None
        } else {
            Some(req.rule_type.as_str())
        };
        let subject = if req.subject.is_empty() {
            None
        } else {
            Some(req.subject.as_str())
        };
        let rules = self
            .db
            .list_stack_rules(rule_type, subject)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListRulesResponse {
            rules: rules.into_iter().map(Into::into).collect(),
        }))
    }

    async fn upsert_rule(
        &self,
        request: Request<proto::UpsertRuleRequest>,
    ) -> Result<Response<proto::UpsertRuleResponse>, Status> {
        let req = request.into_inner();
        if req.rule_type.is_empty() || req.subject.is_empty() || req.content.is_empty() {
            return Err(Status::invalid_argument(
                "rule_type, subject, and content are required",
            ));
        }

        let id = if req.id == 0 { None } else { Some(req.id) };
        let lib_profile_id = if req.lib_profile_id.is_empty() {
            None
        } else {
            Some(req.lib_profile_id.as_str())
        };

        let embedding = if let Some(ref e) = self.embedder {
            let text = format!("{} {} {}", req.rule_type, req.subject, req.content);
            e.embed_query(&text).await.ok()
        } else {
            None
        };
        let embedding_pgvec = embedding.map(|v| crate::embeddings::to_pgvector(&v));

        let rule_id = self
            .db
            .upsert_stack_rule(
                id,
                &req.rule_type,
                &req.subject,
                &req.content,
                lib_profile_id,
                req.priority,
                embedding_pgvec,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let rule = self
            .db
            .get_stack_rule(rule_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("rule vanished after upsert"))?;

        Ok(Response::new(proto::UpsertRuleResponse {
            rule: Some(rule.into()),
        }))
    }

    async fn delete_rule(
        &self,
        request: Request<proto::DeleteRuleRequest>,
    ) -> Result<Response<proto::DeleteRuleResponse>, Status> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .delete_stack_rule(id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !deleted {
            return Err(Status::not_found("rule not found"));
        }
        Ok(Response::new(proto::DeleteRuleResponse { deleted: true }))
    }

    // -----------------------------------------------------------------------
    // Advise / Compare
    // -----------------------------------------------------------------------

    async fn advise(
        &self,
        request: Request<proto::AdviseRequest>,
    ) -> Result<Response<proto::AdviseResponse>, Status> {
        let req = request.into_inner();
        if req.question.is_empty() {
            return Err(Status::invalid_argument("question is required"));
        }

        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;
        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| Status::unavailable("no embedding provider configured"))?;

        let limit = if req.limit == 0 { 5 } else { req.limit };

        let query_vec = embedder
            .embed_query(&req.question)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let query_pgvec = crate::embeddings::to_pgvector(&query_vec);

        let results = self
            .db
            .search_architect_by_vector(&query_pgvec, limit)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut relevant_libs: Vec<crate::db::ArchitectSearchResult> = Vec::new();
        let mut relevant_rules: Vec<crate::db::ArchitectSearchResult> = Vec::new();
        let mut context = String::new();

        use crate::db::ArchitectResultKind;
        for r in results {
            match r.kind {
                ArchitectResultKind::LibProfile => {
                    context.push_str(&format!("### Library: {}\n{}\n\n", r.id, r.text));
                    relevant_libs.push(r);
                }
                ArchitectResultKind::StackRule => {
                    context.push_str(&format!("### Stack Rule #{}\n{}\n\n", r.id, r.text));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Cheatsheet => {
                    let text = if r.text.len() > 1500 {
                        format!("{}...", &r.text[..1500])
                    } else {
                        r.text.clone()
                    };
                    context.push_str(&format!(
                        "### Repo Cheatsheet ({})\n{}\n\n",
                        r.id, text
                    ));
                    relevant_libs.push(r);
                }
                ArchitectResultKind::ProjectProfile => {
                    context.push_str(&format!(
                        "### Project Profile ({})\n{}\n\n",
                        r.id, r.text
                    ));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Decision => {
                    let warning = if r.text.contains("(status: reverted)") {
                        " ⚠ REVERTED"
                    } else {
                        ""
                    };
                    context.push_str(&format!(
                        "### Architecture Decision #{}{}\n{}\n\n",
                        r.id, warning, r.text
                    ));
                    relevant_rules.push(r);
                }
                ArchitectResultKind::Pattern => {
                    context.push_str(&format!(
                        "### Architecture Pattern #{}\n{}\n\n",
                        r.id, r.text
                    ));
                    relevant_libs.push(r);
                }
            }
        }

        let user_message = if context.is_empty() {
            format!("Question: {}\n\nNo relevant context found in the knowledge base. Answer based on your general knowledge.", req.question)
        } else {
            format!(
                "Question: {}\n\n## Knowledge Base Context\n{}",
                req.question, context
            )
        };

        let executor = LlmExecutor::new(&llm_client);
        let user_msgs = [llm_ai::CompletionMessage::new(llm_ai::Role::User, &user_message)];
        let resp = executor.run(&PROMPT_ARCHITECT_ADVISE, &user_msgs)
            .await
            .map_err(|e| Status::internal(format!("LLM error: {e}")))?;

        Ok(Response::new(proto::AdviseResponse {
            answer: resp.content,
            relevant_libs: relevant_libs.into_iter().map(Into::into).collect(),
            relevant_rules: relevant_rules.into_iter().map(Into::into).collect(),
        }))
    }

    async fn compare_libs(
        &self,
        request: Request<proto::CompareLibsRequest>,
    ) -> Result<Response<proto::CompareLibsResponse>, Status> {
        let req = request.into_inner();
        if req.lib_ids.len() < 2 {
            return Err(Status::invalid_argument("at least 2 lib_ids required"));
        }

        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| Status::unavailable("no LLM provider configured"))?;

        let comparison =
            crate::architect::compare_libs(&self.db, llm_client, &req.lib_ids, &req.criteria)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(proto::CompareLibsResponse { comparison }))
    }

    // -----------------------------------------------------------------------
    // Projects
    // -----------------------------------------------------------------------

    async fn list_projects(
        &self,
        _request: Request<proto::ListProjectsRequest>,
    ) -> Result<Response<proto::ListProjectsResponse>, Status> {
        let projects = self
            .db
            .list_project_profiles()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListProjectsResponse {
            projects: projects.into_iter().map(Into::into).collect(),
        }))
    }

    async fn create_project(
        &self,
        request: Request<proto::CreateProjectRequest>,
    ) -> Result<Response<proto::CreateProjectResponse>, Status> {
        let req = request.into_inner();
        if req.id.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument("id and name are required"));
        }

        let repo_id = if req.repo_id.is_empty() {
            None
        } else {
            Some(req.repo_id.as_str())
        };
        let description = if req.description.is_empty() {
            ""
        } else {
            &req.description
        };
        let stack: serde_json::Value = if req.stack.is_empty() {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::from_str(&req.stack)
                .map_err(|e| Status::invalid_argument(format!("invalid stack JSON: {e}")))?
        };
        let constraints = if req.constraints.is_empty() {
            ""
        } else {
            &req.constraints
        };
        let code_style = if req.code_style.is_empty() {
            ""
        } else {
            &req.code_style
        };

        let embedding = if let Some(ref e) = self.embedder {
            let text = format!("{} {} {}", req.name, description, constraints);
            e.embed_query(&text).await.ok()
        } else {
            None
        };
        let embedding_pgvec = embedding.map(|v| crate::embeddings::to_pgvector(&v));

        self.db
            .upsert_project_profile(
                &req.id,
                repo_id,
                &req.name,
                description,
                &stack,
                constraints,
                code_style,
                embedding_pgvec,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let project = self
            .db
            .get_project_profile(&req.id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("project profile vanished after creation"))?;

        Ok(Response::new(proto::CreateProjectResponse {
            project: Some(project.into()),
        }))
    }

    async fn get_project(
        &self,
        request: Request<proto::GetProjectRequest>,
    ) -> Result<Response<proto::GetProjectResponse>, Status> {
        let id = request.into_inner().id;
        let project = self
            .db
            .get_project_profile(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("project profile not found"))?;
        Ok(Response::new(proto::GetProjectResponse {
            project: Some(project.into()),
        }))
    }

    async fn delete_project(
        &self,
        request: Request<proto::DeleteProjectRequest>,
    ) -> Result<Response<proto::DeleteProjectResponse>, Status> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .delete_project_profile(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !deleted {
            return Err(Status::not_found("project profile not found"));
        }
        Ok(Response::new(proto::DeleteProjectResponse { deleted: true }))
    }

    // -----------------------------------------------------------------------
    // Decisions
    // -----------------------------------------------------------------------

    async fn create_decision(
        &self,
        request: Request<proto::CreateDecisionRequest>,
    ) -> Result<Response<proto::CreateDecisionResponse>, Status> {
        let req = request.into_inner();
        if req.title.is_empty() || req.choice.is_empty() {
            return Err(Status::invalid_argument("title and choice are required"));
        }

        let project_profile_id = if req.project_profile_id.is_empty() {
            None
        } else {
            Some(req.project_profile_id.as_str())
        };

        let embedding = if let Some(ref e) = self.embedder {
            let text = format!("{} {} {}", req.title, req.choice, req.reasoning);
            e.embed_query(&text).await.ok()
        } else {
            None
        };
        let embedding_pgvec = embedding.map(|v| crate::embeddings::to_pgvector(&v));

        let decision_id = self
            .db
            .create_arch_decision(
                project_profile_id,
                &req.title,
                &req.context,
                &req.choice,
                &req.alternatives,
                &req.reasoning,
                embedding_pgvec,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let decision = self
            .db
            .get_arch_decision(decision_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("decision vanished after creation"))?;

        Ok(Response::new(proto::CreateDecisionResponse {
            decision: Some(decision.into()),
        }))
    }

    async fn list_decisions(
        &self,
        request: Request<proto::ListDecisionsRequest>,
    ) -> Result<Response<proto::ListDecisionsResponse>, Status> {
        let req = request.into_inner();
        let project_profile_id = if req.project_profile_id.is_empty() {
            None
        } else {
            Some(req.project_profile_id.as_str())
        };
        let status = if req.status.is_empty() {
            None
        } else {
            Some(req.status.as_str())
        };
        let decisions = self
            .db
            .list_arch_decisions(project_profile_id, status)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListDecisionsResponse {
            decisions: decisions.into_iter().map(Into::into).collect(),
        }))
    }

    async fn get_decision(
        &self,
        request: Request<proto::GetDecisionRequest>,
    ) -> Result<Response<proto::GetDecisionResponse>, Status> {
        let id = request.into_inner().id;
        let decision = self
            .db
            .get_arch_decision(id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("decision not found"))?;
        Ok(Response::new(proto::GetDecisionResponse {
            decision: Some(decision.into()),
        }))
    }

    async fn update_decision(
        &self,
        request: Request<proto::UpdateDecisionRequest>,
    ) -> Result<Response<proto::UpdateDecisionResponse>, Status> {
        let req = request.into_inner();
        let outcome = if req.outcome.is_empty() {
            None
        } else {
            Some(req.outcome.as_str())
        };
        let status = if req.status.is_empty() {
            None
        } else {
            Some(req.status.as_str())
        };
        let updated = self
            .db
            .update_arch_decision(req.id, outcome, status, None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !updated {
            return Err(Status::not_found("decision not found"));
        }

        let decision = self
            .db
            .get_arch_decision(req.id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("decision vanished after update"))?;

        Ok(Response::new(proto::UpdateDecisionResponse {
            decision: Some(decision.into()),
        }))
    }

    async fn delete_decision(
        &self,
        request: Request<proto::DeleteDecisionRequest>,
    ) -> Result<Response<proto::DeleteDecisionResponse>, Status> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .delete_arch_decision(id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !deleted {
            return Err(Status::not_found("decision not found"));
        }
        Ok(Response::new(proto::DeleteDecisionResponse { deleted: true }))
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    async fn list_patterns(
        &self,
        request: Request<proto::ListPatternsRequest>,
    ) -> Result<Response<proto::ListPatternsResponse>, Status> {
        let req = request.into_inner();
        let category = if req.category.is_empty() {
            None
        } else {
            Some(req.category.as_str())
        };
        let patterns = self
            .db
            .list_arch_patterns(category)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::ListPatternsResponse {
            patterns: patterns.into_iter().map(Into::into).collect(),
        }))
    }

    async fn create_pattern(
        &self,
        request: Request<proto::CreatePatternRequest>,
    ) -> Result<Response<proto::CreatePatternResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() || req.pattern_text.is_empty() {
            return Err(Status::invalid_argument(
                "name and pattern_text are required",
            ));
        }

        let category = if req.category.is_empty() {
            "general"
        } else {
            &req.category
        };
        let description = if req.description.is_empty() {
            ""
        } else {
            &req.description
        };

        let embedding = if let Some(ref e) = self.embedder {
            let text = format!("{} {} {}", req.name, description, req.pattern_text);
            e.embed_query(&text).await.ok()
        } else {
            None
        };
        let embedding_pgvec = embedding.map(|v| crate::embeddings::to_pgvector(&v));

        let pattern_id = self
            .db
            .create_arch_pattern(
                &req.name,
                category,
                description,
                &req.libs_involved,
                &req.pattern_text,
                "manual",
                embedding_pgvec,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let pattern = self
            .db
            .get_arch_pattern(pattern_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::internal("pattern vanished after creation"))?;

        Ok(Response::new(proto::CreatePatternResponse {
            pattern: Some(pattern.into()),
        }))
    }

    async fn get_pattern(
        &self,
        request: Request<proto::GetPatternRequest>,
    ) -> Result<Response<proto::GetPatternResponse>, Status> {
        let id = request.into_inner().id;
        let pattern = self
            .db
            .get_arch_pattern(id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("pattern not found"))?;
        Ok(Response::new(proto::GetPatternResponse {
            pattern: Some(pattern.into()),
        }))
    }

    async fn delete_pattern(
        &self,
        request: Request<proto::DeletePatternRequest>,
    ) -> Result<Response<proto::DeletePatternResponse>, Status> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .delete_arch_pattern(id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        if !deleted {
            return Err(Status::not_found("pattern not found"));
        }
        Ok(Response::new(proto::DeletePatternResponse { deleted: true }))
    }
}
