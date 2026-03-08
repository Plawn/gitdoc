use anyhow::{Result, anyhow};
use futures_util::StreamExt;

use crate::types::*;

pub struct GitdocClient {
    http: reqwest::Client,
    base_url: String,
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

impl GitdocClient {
    pub fn new(server_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: server_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn health(&self) -> Result<String> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.text().await?)
    }

    pub async fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let resp = self
            .http
            .get(format!("{}/repos", self.base_url))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_repo(&self, id: &str) -> Result<RepoDetail> {
        let resp = self
            .http
            .get(format!("{}/repos/{}", self.base_url, id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn create_repo(
        &self,
        id: &str,
        name: &str,
        url: &str,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "id": id, "name": name, "url": url });
        let resp = self
            .http
            .post(format!("{}/repos", self.base_url))
            .json(&body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn fetch_repo(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/repos/{}/fetch", self.base_url, repo_id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn index_repo(
        &self,
        repo_id: &str,
        commit: &str,
        label: Option<&str>,
        fetch: bool,
    ) -> Result<IndexResult> {
        let mut body = serde_json::json!({ "commit": commit, "fetch": fetch });
        if let Some(l) = label {
            body["label"] = serde_json::Value::String(l.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/repos/{}/index", self.base_url, repo_id))
            .json(&body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_overview(&self, snapshot_id: i64) -> Result<OverviewResponse> {
        let resp = self
            .http
            .get(format!("{}/snapshots/{}/overview", self.base_url, snapshot_id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn list_docs(&self, snapshot_id: i64) -> Result<Vec<DocRow>> {
        let resp = self
            .http
            .get(format!("{}/snapshots/{}/docs", self.base_url, snapshot_id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn read_doc(&self, snapshot_id: i64, path: &str) -> Result<DocContent> {
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/docs/{}",
                self.base_url, snapshot_id, path
            ))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn list_symbols(
        &self,
        snapshot_id: i64,
        kind: Option<&str>,
        visibility: Option<&str>,
        file_path: Option<&str>,
        include_private: Option<bool>,
    ) -> Result<Vec<SymbolRow>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(k) = kind {
            params.push(("kind", k.to_string()));
        }
        if let Some(v) = visibility {
            params.push(("visibility", v.to_string()));
        }
        if let Some(fp) = file_path {
            params.push(("file_path", fp.to_string()));
        }
        if let Some(ip) = include_private {
            params.push(("include_private", ip.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/symbols", self.base_url, snapshot_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_symbol(&self, symbol_id: i64) -> Result<SymbolDetailResponse> {
        let resp = self
            .http
            .get(format!("{}/symbols/{}", self.base_url, symbol_id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_references(
        &self,
        snapshot_id: i64,
        symbol_id: i64,
        direction: Option<&str>,
        kind: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<RefWithSymbol>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(d) = direction {
            params.push(("direction", d.to_string()));
        }
        if let Some(k) = kind {
            params.push(("kind", k.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/symbols/{}/references", self.base_url, snapshot_id, symbol_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_implementations(
        &self,
        snapshot_id: i64,
        symbol_id: i64,
    ) -> Result<Vec<RefWithSymbol>> {
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/symbols/{}/implementations",
                self.base_url, snapshot_id, symbol_id
            ))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn search_docs(
        &self,
        snapshot_id: i64,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<DocSearchResult>> {
        let mut params = vec![("q", query.to_string())];
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/search/docs",
                self.base_url, snapshot_id
            ))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn search_symbols(
        &self,
        snapshot_id: i64,
        query: &str,
        kind: Option<&str>,
        visibility: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SymbolSearchResult>> {
        let mut params = vec![("q", query.to_string())];
        if let Some(k) = kind {
            params.push(("kind", k.to_string()));
        }
        if let Some(v) = visibility {
            params.push(("visibility", v.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/search/symbols",
                self.base_url, snapshot_id
            ))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn semantic_search(
        &self,
        snapshot_id: i64,
        query: &str,
        scope: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SemanticSearchResult>> {
        let mut params = vec![("q", query.to_string())];
        if let Some(s) = scope {
            params.push(("scope", s.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/search/semantic",
                self.base_url, snapshot_id
            ))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn diff_symbols(
        &self,
        from_id: i64,
        to_id: i64,
        kind: Option<&str>,
        include_private: Option<bool>,
    ) -> Result<DiffResponse> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(k) = kind {
            params.push(("kind", k.to_string()));
        }
        if let Some(ip) = include_private {
            params.push(("include_private", ip.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/diff/{}", self.base_url, from_id, to_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_public_api(
        &self,
        snapshot_id: i64,
        module_path: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<PublicApiResponse> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(mp) = module_path {
            params.push(("module_path", mp.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        if let Some(o) = offset {
            params.push(("offset", o.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/public_api", self.base_url, snapshot_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_module_tree(
        &self,
        snapshot_id: i64,
        depth: Option<usize>,
        include_signatures: Option<bool>,
    ) -> Result<ModuleTreeResponse> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(d) = depth {
            params.push(("depth", d.to_string()));
        }
        if let Some(is) = include_signatures {
            params.push(("include_signatures", is.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/module_tree", self.base_url, snapshot_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_type_context(
        &self,
        snapshot_id: i64,
        symbol_id: i64,
    ) -> Result<TypeContextResponse> {
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/symbols/{}/type_context",
                self.base_url, snapshot_id, symbol_id
            ))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_examples(
        &self,
        snapshot_id: i64,
        symbol_id: i64,
    ) -> Result<ExamplesResponse> {
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/symbols/{}/examples",
                self.base_url, snapshot_id, symbol_id
            ))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn summarize(
        &self,
        snapshot_id: i64,
        scope: &str,
    ) -> Result<SummarizeResponse> {
        let resp = self
            .http
            .post(format!("{}/snapshots/{}/summarize", self.base_url, snapshot_id))
            .query(&[("scope", scope)])
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn explain(
        &self,
        snapshot_id: i64,
        query: &str,
        synthesize: Option<bool>,
        limit: Option<usize>,
    ) -> Result<serde_json::Value> {
        let mut params = vec![("q", query.to_string())];
        if let Some(s) = synthesize {
            params.push(("synthesize", s.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/explain",
                self.base_url, snapshot_id
            ))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn converse(
        &self,
        snapshot_id: i64,
        question: &str,
        conversation_id: Option<i64>,
        limit: Option<usize>,
        detail_level: Option<&str>,
    ) -> Result<ConversationResponse> {
        let mut body = serde_json::json!({ "q": question });
        if let Some(cid) = conversation_id {
            body["conversation_id"] = serde_json::Value::Number(cid.into());
        }
        if let Some(l) = limit {
            body["limit"] = serde_json::Value::Number(l.into());
        }
        if let Some(dl) = detail_level {
            body["detail_level"] = serde_json::Value::String(dl.to_string());
        }
        let resp = self
            .http
            .post(format!(
                "{}/snapshots/{}/converse",
                self.base_url, snapshot_id
            ))
            .json(&body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_conversation(
        &self,
        snapshot_id: i64,
        conversation_id: i64,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(format!(
                "{}/snapshots/{}/conversations/{}",
                self.base_url, snapshot_id, conversation_id
            ))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_cheatsheet(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}/repos/{}/cheatsheet", self.base_url, repo_id))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(serde_json::json!({}));
        }
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn generate_cheatsheet(
        &self,
        repo_id: &str,
        snapshot_id: i64,
        trigger: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut body = serde_json::json!({ "snapshot_id": snapshot_id });
        if let Some(t) = trigger {
            body["trigger"] = serde_json::Value::String(t.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/repos/{}/cheatsheet", self.base_url, repo_id))
            .json(&body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn list_cheatsheet_patches(
        &self,
        repo_id: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        if let Some(o) = offset {
            params.push(("offset", o.to_string()));
        }
        let resp = self.http
            .get(format!("{}/repos/{}/cheatsheet/patches", self.base_url, repo_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_cheatsheet_patch(
        &self,
        repo_id: &str,
        patch_id: i64,
    ) -> Result<serde_json::Value> {
        let resp = self.http
            .get(format!("{}/repos/{}/cheatsheet/patches/{}", self.base_url, repo_id, patch_id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_summary(
        &self,
        snapshot_id: i64,
        scope: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(s) = scope {
            params.push(("scope", s.to_string()));
        }
        let resp = self.http
            .get(format!("{}/snapshots/{}/summary", self.base_url, snapshot_id))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    /// Stream cheatsheet generation progress via SSE.
    /// Returns a receiver that yields progress events.
    pub async fn stream_generate_cheatsheet(
        &self,
        repo_id: &str,
        snapshot_id: i64,
        trigger: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<CheatsheetProgressEvent>> {
        let body = serde_json::json!({
            "snapshot_id": snapshot_id,
            "trigger": trigger,
        });
        let resp = self
            .http
            .post(format!("{}/repos/{}/cheatsheet/stream", self.base_url, repo_id))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status.as_u16(), body));
        }

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

                // Parse SSE events: split on double newline
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

    // --- Architect methods ---

    pub async fn list_lib_profiles(&self, category: Option<&str>) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(cat) = category {
            params.push(("category", cat.to_string()));
        }
        let resp = self.http
            .get(format!("{}/architect/libs", self.base_url))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_lib_profile(&self, id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}/architect/libs/{}", self.base_url, id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn create_lib_profile(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/architect/libs", self.base_url))
            .json(body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn generate_lib_profile(&self, id: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/architect/libs/{}/generate", self.base_url, id))
            .json(body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_lib_profile(&self, id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(format!("{}/architect/libs/{}", self.base_url, id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn list_stack_rules(
        &self,
        rule_type: Option<&str>,
        subject: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(rt) = rule_type {
            params.push(("rule_type", rt.to_string()));
        }
        if let Some(sub) = subject {
            params.push(("subject", sub.to_string()));
        }
        let resp = self.http
            .get(format!("{}/architect/rules", self.base_url))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn upsert_stack_rule(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/architect/rules", self.base_url))
            .json(body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_stack_rule(&self, id: i64) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(format!("{}/architect/rules/{}", self.base_url, id))
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn architect_advise(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/architect/advise", self.base_url))
            .json(body)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // --- Project Profiles ---

    pub async fn list_project_profiles(&self) -> Result<serde_json::Value> {
        let resp = self.http.get(format!("{}/architect/projects", self.base_url)).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn create_project_profile(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.http.post(format!("{}/architect/projects", self.base_url)).json(body).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_project_profile(&self, id: &str) -> Result<serde_json::Value> {
        let resp = self.http.get(format!("{}/architect/projects/{}", self.base_url, id)).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_project_profile(&self, id: &str) -> Result<serde_json::Value> {
        let resp = self.http.delete(format!("{}/architect/projects/{}", self.base_url, id)).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // --- Architecture Decisions ---

    pub async fn create_decision(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.http.post(format!("{}/architect/decisions", self.base_url)).json(body).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn list_decisions(
        &self,
        project_profile_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(pid) = project_profile_id {
            params.push(("project_profile_id", pid.to_string()));
        }
        if let Some(s) = status {
            params.push(("status", s.to_string()));
        }
        let resp = self.http
            .get(format!("{}/architect/decisions", self.base_url))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn update_decision(&self, id: i64, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.http.put(format!("{}/architect/decisions/{}", self.base_url, id)).json(body).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // --- Compare Libs ---

    pub async fn compare_libs(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.http.post(format!("{}/architect/compare", self.base_url)).json(body).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // --- Architecture Patterns ---

    pub async fn list_patterns(&self, category: Option<&str>) -> Result<serde_json::Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(cat) = category {
            params.push(("category", cat.to_string()));
        }
        let resp = self.http
            .get(format!("{}/architect/patterns", self.base_url))
            .query(&params)
            .send()
            .await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn create_pattern(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.http.post(format!("{}/architect/patterns", self.base_url)).json(body).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_pattern(&self, id: i64) -> Result<serde_json::Value> {
        let resp = self.http.get(format!("{}/architect/patterns/{}", self.base_url, id)).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_pattern(&self, id: i64) -> Result<serde_json::Value> {
        let resp = self.http.delete(format!("{}/architect/patterns/{}", self.base_url, id)).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }
}

pub struct CheatsheetProgressEvent {
    pub stage: String,
    pub message: String,
}
