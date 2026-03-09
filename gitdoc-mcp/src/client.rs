use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use gitdoc_api_types::requests::*;

use crate::config::BasicAuth;
use crate::types::*;

pub struct GitdocClient {
    http: reqwest::Client,
    base_url: String,
}

impl GitdocClient {
    pub fn new(server_url: &str, basic_auth: Option<&BasicAuth>) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(auth) = basic_auth {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", auth.username, auth.password));
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Basic {encoded}"))
                    .expect("invalid basic auth credentials"),
            );
        }
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            base_url: server_url.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Send a request, check for HTTP errors, and deserialize the JSON body.
    async fn send<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        let resp = req.send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json().await?)
    }

    /// Send a request, check for HTTP errors, and return the body as text.
    async fn send_text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        let resp = req.send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.text().await?)
    }

    /// Extract a readable error message from a non-2xx HTTP response.
    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = json.get("error").and_then(|v| v.as_str()) {
                return Err(anyhow!("HTTP {}: {}", status.as_u16(), msg));
            }
        }
        Err(anyhow!("HTTP {}: {}", status.as_u16(), body))
    }

    // -----------------------------------------------------------------------
    // Health
    // -----------------------------------------------------------------------

    pub async fn health(&self) -> Result<String> {
        self.send_text(self.http.get(self.url("/health"))).await
    }

    // -----------------------------------------------------------------------
    // Repos
    // -----------------------------------------------------------------------

    pub async fn list_repos(&self) -> Result<Vec<RepoRow>> {
        self.send(self.http.get(self.url("/repos"))).await
    }

    pub async fn get_repo(&self, id: &str) -> Result<RepoDetail> {
        self.send(self.http.get(self.url(&format!("/repos/{id}")))).await
    }

    pub async fn create_repo(&self, id: &str, name: &str, url: &str) -> Result<serde_json::Value> {
        let body = CreateRepoBody {
            id: id.to_string(),
            name: name.to_string(),
            url: url.to_string(),
        };
        self.send(self.http.post(self.url("/repos")).json(&body)).await
    }

    pub async fn fetch_repo(&self, repo_id: &str) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url(&format!("/repos/{repo_id}/fetch")))).await
    }

    pub async fn index_repo(
        &self,
        repo_id: &str,
        commit: &str,
        label: Option<&str>,
        fetch: bool,
    ) -> Result<IndexResult> {
        let body = IndexBody {
            commit: commit.to_string(),
            label: label.map(|s| s.to_string()),
            fetch,
        };
        self.send(self.http.post(self.url(&format!("/repos/{repo_id}/index"))).json(&body)).await
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub async fn get_overview(&self, snapshot_id: i64) -> Result<OverviewResponse> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/overview")))).await
    }

    pub async fn list_docs(&self, snapshot_id: i64) -> Result<Vec<DocRow>> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/docs")))).await
    }

    pub async fn read_doc(&self, snapshot_id: i64, path: &str) -> Result<DocContent> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/docs/{path}")))).await
    }

    // -----------------------------------------------------------------------
    // Symbols
    // -----------------------------------------------------------------------

    pub async fn list_symbols(
        &self,
        snapshot_id: i64,
        kind: Option<&str>,
        visibility: Option<&str>,
        file_path: Option<&str>,
        include_private: Option<bool>,
    ) -> Result<Vec<SymbolRow>> {
        let query = SymbolQuery {
            kind: kind.map(|s| s.to_string()),
            visibility: visibility.map(|s| s.to_string()),
            file_path: file_path.map(|s| s.to_string()),
            include_private,
        };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/symbols"))).query(&query)).await
    }

    pub async fn get_symbol(&self, symbol_id: i64) -> Result<SymbolDetailResponse> {
        self.send(self.http.get(self.url(&format!("/symbols/{symbol_id}")))).await
    }

    pub async fn get_references(
        &self,
        snapshot_id: i64,
        symbol_id: i64,
        direction: Option<&str>,
        kind: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<RefWithSymbol>> {
        let query = RefQuery {
            direction: direction.map(|s| s.to_string()),
            kind: kind.map(|s| s.to_string()),
            limit,
        };
        self.send(
            self.http
                .get(self.url(&format!("/snapshots/{snapshot_id}/symbols/{symbol_id}/references")))
                .query(&query),
        ).await
    }

    pub async fn get_implementations(&self, snapshot_id: i64, symbol_id: i64) -> Result<Vec<RefWithSymbol>> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/symbols/{symbol_id}/implementations")))).await
    }

    pub async fn get_type_context(&self, snapshot_id: i64, symbol_id: i64) -> Result<TypeContextResponse> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/symbols/{symbol_id}/type_context")))).await
    }

    pub async fn get_examples(&self, snapshot_id: i64, symbol_id: i64) -> Result<ExamplesResponse> {
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/symbols/{symbol_id}/examples")))).await
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    pub async fn search_docs(&self, snapshot_id: i64, query: &str, limit: Option<usize>) -> Result<Vec<DocSearchResult>> {
        let q = DocSearchQuery { q: query.to_string(), limit };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/search/docs"))).query(&q)).await
    }

    pub async fn search_symbols(
        &self,
        snapshot_id: i64,
        query: &str,
        kind: Option<&str>,
        visibility: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SymbolSearchResult>> {
        let q = SymbolSearchQuery {
            q: query.to_string(),
            kind: kind.map(|s| s.to_string()),
            visibility: visibility.map(|s| s.to_string()),
            limit,
        };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/search/symbols"))).query(&q)).await
    }

    pub async fn semantic_search(
        &self,
        snapshot_id: i64,
        query: &str,
        scope: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SemanticSearchResult>> {
        let q = SemanticSearchQuery {
            q: query.to_string(),
            scope: scope.map(|s| s.to_string()),
            limit,
        };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/search/semantic"))).query(&q)).await
    }

    // -----------------------------------------------------------------------
    // Diff
    // -----------------------------------------------------------------------

    pub async fn diff_symbols(
        &self,
        from_id: i64,
        to_id: i64,
        kind: Option<&str>,
        include_private: Option<bool>,
    ) -> Result<DiffResponse> {
        let q = DiffQuery { kind: kind.map(|s| s.to_string()), include_private };
        self.send(self.http.get(self.url(&format!("/snapshots/{from_id}/diff/{to_id}"))).query(&q)).await
    }

    // -----------------------------------------------------------------------
    // Public API / Module tree
    // -----------------------------------------------------------------------

    pub async fn get_public_api(
        &self,
        snapshot_id: i64,
        module_path: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<PublicApiResponse> {
        let q = PublicApiQuery { module_path: module_path.map(|s| s.to_string()), limit, offset };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/public_api"))).query(&q)).await
    }

    pub async fn get_module_tree(
        &self,
        snapshot_id: i64,
        depth: Option<usize>,
        include_signatures: Option<bool>,
    ) -> Result<ModuleTreeResponse> {
        let q = ModuleTreeQuery { depth, include_signatures };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/module_tree"))).query(&q)).await
    }

    // -----------------------------------------------------------------------
    // Summaries / Explain
    // -----------------------------------------------------------------------

    pub async fn summarize(&self, snapshot_id: i64, scope: &str) -> Result<SummarizeResponse> {
        let q = SummarizeQuery { scope: scope.to_string() };
        self.send(self.http.post(self.url(&format!("/snapshots/{snapshot_id}/summarize"))).query(&q)).await
    }

    pub async fn get_summary(&self, snapshot_id: i64, scope: Option<&str>) -> Result<serde_json::Value> {
        let q = SummaryQuery { scope: scope.map(|s| s.to_string()) };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/summary"))).query(&q)).await
    }

    pub async fn explain(
        &self,
        snapshot_id: i64,
        query: &str,
        synthesize: Option<bool>,
        limit: Option<usize>,
    ) -> Result<serde_json::Value> {
        let q = ExplainQuery { q: query.to_string(), synthesize, limit };
        self.send(self.http.get(self.url(&format!("/snapshots/{snapshot_id}/explain"))).query(&q)).await
    }

    // -----------------------------------------------------------------------
    // Converse
    // -----------------------------------------------------------------------

    pub async fn converse(
        &self,
        snapshot_id: i64,
        question: &str,
        conversation_id: Option<i64>,
        limit: Option<usize>,
        detail_level: Option<&str>,
    ) -> Result<ConversationResponse> {
        let body = ConverseRequest {
            q: question.to_string(),
            conversation_id,
            limit,
            detail_level: detail_level.map(|s| s.to_string()),
        };
        self.send(self.http.post(self.url(&format!("/snapshots/{snapshot_id}/converse"))).json(&body)).await
    }

    pub async fn delete_conversation(&self, snapshot_id: i64, conversation_id: i64) -> Result<serde_json::Value> {
        self.send(self.http.delete(self.url(&format!("/snapshots/{snapshot_id}/conversations/{conversation_id}")))).await
    }

    // -----------------------------------------------------------------------
    // Cheatsheet
    // -----------------------------------------------------------------------

    pub async fn get_cheatsheet(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self.http.get(self.url(&format!("/repos/{repo_id}/cheatsheet"))).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(serde_json::json!({}));
        }
        let resp = Self::check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn generate_cheatsheet(
        &self,
        repo_id: &str,
        snapshot_id: i64,
        trigger: Option<&str>,
    ) -> Result<serde_json::Value> {
        let body = GenerateCheatsheetRequest {
            snapshot_id,
            trigger: trigger.map(|s| s.to_string()),
        };
        self.send(self.http.post(self.url(&format!("/repos/{repo_id}/cheatsheet"))).json(&body)).await
    }

    pub async fn list_cheatsheet_patches(&self, repo_id: &str, limit: Option<i64>, offset: Option<i64>) -> Result<serde_json::Value> {
        let q = PatchListQuery { limit, offset };
        self.send(self.http.get(self.url(&format!("/repos/{repo_id}/cheatsheet/patches"))).query(&q)).await
    }

    pub async fn get_cheatsheet_patch(&self, repo_id: &str, patch_id: i64) -> Result<serde_json::Value> {
        self.send(self.http.get(self.url(&format!("/repos/{repo_id}/cheatsheet/patches/{patch_id}")))).await
    }

    /// Stream cheatsheet generation progress via SSE.
    pub async fn stream_generate_cheatsheet(
        &self,
        repo_id: &str,
        snapshot_id: i64,
        trigger: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<CheatsheetProgressEvent>> {
        let body = GenerateCheatsheetRequest {
            snapshot_id,
            trigger: Some(trigger.to_string()),
        };
        let resp = self
            .http
            .post(self.url(&format!("/repos/{repo_id}/cheatsheet/stream")))
            .json(&body)
            .send()
            .await?;
        let resp = Self::check_response(resp).await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<CheatsheetProgressEvent>(16);

        let mut stream = resp.bytes_stream();
        tokio::spawn(async move {
            let mut buf = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => break,
                };
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buf.find("\n\n") {
                    let event_block = buf[..pos].to_string();
                    buf = buf[pos + 2..].to_string();

                    for line in event_block.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            let data = data.trim();
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                let event = CheatsheetProgressEvent {
                                    stage: json.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    message: json.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                };
                                if tx.send(event).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    // -----------------------------------------------------------------------
    // Architect — Libs
    // -----------------------------------------------------------------------

    pub async fn list_lib_profiles(&self, category: Option<&str>) -> Result<serde_json::Value> {
        let q = ListLibsQuery { category: category.map(|s| s.to_string()) };
        self.send(self.http.get(self.url("/architect/libs")).query(&q)).await
    }

    pub async fn get_lib_profile(&self, id: &str) -> Result<serde_json::Value> {
        self.send(self.http.get(self.url(&format!("/architect/libs/{id}")))).await
    }

    pub async fn create_lib_profile(&self, body: &CreateLibRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/libs")).json(body)).await
    }

    pub async fn generate_lib_profile(&self, id: &str, body: &GenerateLibProfileRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url(&format!("/architect/libs/{id}/generate"))).json(body)).await
    }

    pub async fn delete_lib_profile(&self, id: &str) -> Result<serde_json::Value> {
        self.send(self.http.delete(self.url(&format!("/architect/libs/{id}")))).await
    }

    // -----------------------------------------------------------------------
    // Architect — Rules
    // -----------------------------------------------------------------------

    pub async fn list_stack_rules(&self, rule_type: Option<&str>, subject: Option<&str>) -> Result<serde_json::Value> {
        let q = ListRulesQuery {
            rule_type: rule_type.map(|s| s.to_string()),
            subject: subject.map(|s| s.to_string()),
        };
        self.send(self.http.get(self.url("/architect/rules")).query(&q)).await
    }

    pub async fn upsert_stack_rule(&self, body: &UpsertRuleRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/rules")).json(body)).await
    }

    pub async fn delete_stack_rule(&self, id: i64) -> Result<serde_json::Value> {
        self.send(self.http.delete(self.url(&format!("/architect/rules/{id}")))).await
    }

    // -----------------------------------------------------------------------
    // Architect — Advise / Compare
    // -----------------------------------------------------------------------

    pub async fn architect_advise(&self, body: &AdviseRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/advise")).json(body)).await
    }

    pub async fn compare_libs(&self, body: &CompareLibsRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/compare")).json(body)).await
    }

    // -----------------------------------------------------------------------
    // Architect — Projects
    // -----------------------------------------------------------------------

    pub async fn list_project_profiles(&self) -> Result<serde_json::Value> {
        self.send(self.http.get(self.url("/architect/projects"))).await
    }

    pub async fn create_project_profile(&self, body: &CreateProjectProfileRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/projects")).json(body)).await
    }

    pub async fn get_project_profile(&self, id: &str) -> Result<serde_json::Value> {
        self.send(self.http.get(self.url(&format!("/architect/projects/{id}")))).await
    }

    pub async fn delete_project_profile(&self, id: &str) -> Result<serde_json::Value> {
        self.send(self.http.delete(self.url(&format!("/architect/projects/{id}")))).await
    }

    // -----------------------------------------------------------------------
    // Architect — Decisions
    // -----------------------------------------------------------------------

    pub async fn create_decision(&self, body: &CreateDecisionRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/decisions")).json(body)).await
    }

    pub async fn list_decisions(&self, project_profile_id: Option<&str>, status: Option<&str>) -> Result<serde_json::Value> {
        let q = ListDecisionsQuery {
            project_profile_id: project_profile_id.map(|s| s.to_string()),
            status: status.map(|s| s.to_string()),
        };
        self.send(self.http.get(self.url("/architect/decisions")).query(&q)).await
    }

    pub async fn update_decision(&self, id: i64, body: &UpdateDecisionRequest) -> Result<serde_json::Value> {
        self.send(self.http.put(self.url(&format!("/architect/decisions/{id}"))).json(body)).await
    }

    // -----------------------------------------------------------------------
    // Architect — Patterns
    // -----------------------------------------------------------------------

    pub async fn list_patterns(&self, category: Option<&str>) -> Result<serde_json::Value> {
        let q = ListPatternsQuery { category: category.map(|s| s.to_string()) };
        self.send(self.http.get(self.url("/architect/patterns")).query(&q)).await
    }

    pub async fn create_pattern(&self, body: &CreatePatternRequest) -> Result<serde_json::Value> {
        self.send(self.http.post(self.url("/architect/patterns")).json(body)).await
    }

    pub async fn get_pattern(&self, id: i64) -> Result<serde_json::Value> {
        self.send(self.http.get(self.url(&format!("/architect/patterns/{id}")))).await
    }

    pub async fn delete_pattern(&self, id: i64) -> Result<serde_json::Value> {
        self.send(self.http.delete(self.url(&format!("/architect/patterns/{id}")))).await
    }
}

pub struct CheatsheetProgressEvent {
    pub stage: String,
    pub message: String,
}
