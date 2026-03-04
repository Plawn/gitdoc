use anyhow::Result;
use serde::{Deserialize, Serialize};

// --- Response types ---

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub repo_id: String,
    pub commit_sha: String,
    pub label: Option<String>,
    pub indexed_at: i64,
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
pub struct RefWithSymbol {
    pub ref_kind: String,
    pub symbol: SymbolRow,
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

    pub async fn index_repo(
        &self,
        repo_id: &str,
        commit: &str,
        label: Option<&str>,
    ) -> Result<IndexResult> {
        let mut body = serde_json::json!({ "commit": commit });
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
}
