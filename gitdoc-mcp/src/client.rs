use anyhow::Result;
use serde::{Deserialize, Serialize};

// --- Response types ---

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub repo_id: String,
    pub commit_sha: String,
    pub label: Option<String>,
    pub indexed_at: String,
    pub status: String,
    pub stats: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoDetail {
    pub repo: RepoRow,
    pub snapshots: Vec<SnapshotRow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IndexResult {
    pub snapshot_id: i64,
    pub commit_sha: String,
    pub stats: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocRow {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocContent {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolRow {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolDetail {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub body: String,
    pub parent_id: Option<i64>,
    pub children_count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolDetailResponse {
    pub symbol: SymbolDetail,
    pub children: Vec<SymbolRow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OverviewResponse {
    pub snapshot: SnapshotRow,
    pub readme: Option<String>,
    pub docs: Vec<DocRow>,
    pub top_level_symbols: Vec<SymbolRow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocSearchResult {
    pub file_path: String,
    pub title: String,
    pub snippets: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SymbolSearchResult {
    pub symbol_id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub file_path: String,
    pub score: f32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RefWithSymbol {
    pub ref_kind: String,
    pub symbol: SymbolRow,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticSearchResult {
    pub source_type: String,
    pub source_id: i64,
    pub score: f32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<SemanticDocHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SemanticSymbolHit>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticDocHit {
    pub file_path: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticSymbolHit {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    pub file_path: String,
    pub line_start: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSymbolEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModifiedFields {
    pub signature: String,
    pub visibility: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModifiedSymbol {
    pub qualified_name: String,
    pub kind: String,
    pub changes: Vec<String>,
    pub from: ModifiedFields,
    pub to: ModifiedFields,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffSummary {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiffResponse {
    pub from_snapshot: i64,
    pub to_snapshot: i64,
    pub added: Vec<DiffSymbolEntry>,
    pub removed: Vec<DiffSymbolEntry>,
    pub modified: Vec<ModifiedSymbol>,
    pub summary: DiffSummary,
}

pub struct GitdocClient {
    http: reqwest::Client,
    base_url: String,
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
            .await?
            .text()
            .await?;
        Ok(resp)
    }

    pub async fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let resp = self
            .http
            .get(format!("{}/repos", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn get_repo(&self, id: &str) -> Result<RepoDetail> {
        let resp = self
            .http
            .get(format!("{}/repos/{}", self.base_url, id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn create_repo(
        &self,
        id: &str,
        name: &str,
        url: Option<&str>,
        path: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut body = serde_json::json!({ "id": id, "name": name });
        if let Some(u) = url {
            body["url"] = serde_json::Value::String(u.to_string());
        }
        if let Some(p) = path {
            body["path"] = serde_json::Value::String(p.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/repos", self.base_url))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn fetch_repo(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/repos/{}/fetch", self.base_url, repo_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
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
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn get_overview(&self, snapshot_id: i64) -> Result<OverviewResponse> {
        let resp = self
            .http
            .get(format!("{}/snapshots/{}/overview", self.base_url, snapshot_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn list_docs(&self, snapshot_id: i64) -> Result<Vec<DocRow>> {
        let resp = self
            .http
            .get(format!("{}/snapshots/{}/docs", self.base_url, snapshot_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn read_doc(&self, snapshot_id: i64, path: &str) -> Result<DocContent> {
        let resp = self
            .http
            .get(format!(
                "{}/snapshots/{}/docs/{}",
                self.base_url, snapshot_id, path
            ))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
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
        let resp = self.http.get(&url).send().await?.json().await?;
        Ok(resp)
    }

    pub async fn get_symbol(&self, symbol_id: i64) -> Result<SymbolDetailResponse> {
        let resp = self
            .http
            .get(format!("{}/symbols/{}", self.base_url, symbol_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
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
        let resp = self.http.get(&url).send().await?.json().await?;
        Ok(resp)
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
            .await?
            .json()
            .await?;
        Ok(resp)
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
            .await?
            .json()
            .await?;
        Ok(resp)
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
            .await?
            .json()
            .await?;
        Ok(resp)
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
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn delete_snapshot(&self, snapshot_id: i64) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(format!("{}/snapshots/{}", self.base_url, snapshot_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    #[allow(dead_code)]
    pub async fn delete_repo(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(format!("{}/repos/{}", self.base_url, repo_id))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn gc(&self) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/admin/gc", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
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
        let resp = self.http.get(&url).send().await?.json().await?;
        Ok(resp)
    }
}
