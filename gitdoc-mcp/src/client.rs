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
        let mut url = format!("{}/snapshots/{}/symbols", self.base_url, snapshot_id);
        let mut params = Vec::new();
        if let Some(k) = kind {
            params.push(format!("kind={}", k));
        }
        if let Some(v) = visibility {
            params.push(format!("visibility={}", v));
        }
        if let Some(fp) = file_path {
            params.push(format!("file_path={}", fp));
        }
        if let Some(ip) = include_private {
            params.push(format!("include_private={}", ip));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
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
        let mut url = format!(
            "{}/snapshots/{}/symbols/{}/references",
            self.base_url, snapshot_id, symbol_id
        );
        let mut params = Vec::new();
        if let Some(d) = direction {
            params.push(format!("direction={}", d));
        }
        if let Some(k) = kind {
            params.push(format!("kind={}", k));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
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
        let mut url = format!("{}/snapshots/{}/diff/{}", self.base_url, from_id, to_id);
        let mut params = Vec::new();
        if let Some(k) = kind {
            params.push(format!("kind={}", k));
        }
        if let Some(ip) = include_private {
            params.push(format!("include_private={}", ip));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
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
        let mut url = format!("{}/snapshots/{}/public_api", self.base_url, snapshot_id);
        let mut params = Vec::new();
        if let Some(mp) = module_path {
            params.push(format!("module_path={}", mp));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_module_tree(
        &self,
        snapshot_id: i64,
        depth: Option<usize>,
        include_signatures: Option<bool>,
    ) -> Result<ModuleTreeResponse> {
        let mut url = format!("{}/snapshots/{}/module_tree", self.base_url, snapshot_id);
        let mut params = Vec::new();
        if let Some(d) = depth {
            params.push(format!("depth={}", d));
        }
        if let Some(is) = include_signatures {
            params.push(format!("include_signatures={}", is));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
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
            .post(format!(
                "{}/snapshots/{}/summarize?scope={}",
                self.base_url, snapshot_id, scope
            ))
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
    ) -> Result<ConversationResponse> {
        let mut body = serde_json::json!({ "q": question });
        if let Some(cid) = conversation_id {
            body["conversation_id"] = serde_json::Value::Number(cid.into());
        }
        if let Some(l) = limit {
            body["limit"] = serde_json::Value::Number(l.into());
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
        let mut url = format!("{}/repos/{}/cheatsheet/patches", self.base_url, repo_id);
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self.http.get(&url).send().await?;
        let resp = check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn get_summary(
        &self,
        snapshot_id: i64,
        scope: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut url = format!("{}/snapshots/{}/summary", self.base_url, snapshot_id);
        if let Some(s) = scope {
            url.push_str(&format!("?scope={}", s));
        }
        let resp = self.http.get(&url).send().await?;
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
                                    patch_id: json.get("patch_id").and_then(|v| v.as_i64()),
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
}

pub struct CheatsheetProgressEvent {
    pub stage: String,
    pub message: String,
    pub patch_id: Option<i64>,
}
