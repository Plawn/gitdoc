use anyhow::{Result, bail};
use std::future::Future;
use std::pin::Pin;

// --- Trait ---

pub trait EmbeddingProvider: Send + Sync {
    fn dimensions(&self) -> usize;
    fn embed_batch(&self, texts: &[String]) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>>;
    fn embed_query(&self, text: &str) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + '_>> {
        let text = text.to_string();
        Box::pin(async move {
            let vecs = self.embed_batch(&[text]).await?;
            vecs.into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("empty embedding response"))
        })
    }
}

// --- Cohere Provider ---

pub struct CohereProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl CohereProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "embed-v4.0".to_string(),
            client: reqwest::Client::new(),
        }
    }
}

impl EmbeddingProvider for CohereProvider {
    fn dimensions(&self) -> usize {
        1024
    }

    fn embed_batch(&self, texts: &[String]) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
        let texts = texts.to_vec();
        Box::pin(async move {
            if texts.is_empty() {
                return Ok(vec![]);
            }
            let body = serde_json::json!({
                "model": self.model,
                "texts": texts,
                "input_type": "search_document",
                "embedding_types": ["float"],
            });
            let resp: serde_json::Value = self.client
                .post("https://api.cohere.com/v2/embed")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let embeddings = resp["embeddings"]["float"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("missing embeddings.float in Cohere response"))?;

            let mut result = Vec::with_capacity(embeddings.len());
            for emb in embeddings {
                let vec: Vec<f32> = emb
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("embedding is not an array"))?
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect();
                result.push(vec);
            }
            Ok(result)
        })
    }

    fn embed_query(&self, text: &str) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + '_>> {
        let text = text.to_string();
        Box::pin(async move {
            let body = serde_json::json!({
                "model": self.model,
                "texts": [text],
                "input_type": "search_query",
                "embedding_types": ["float"],
            });
            let resp: serde_json::Value = self.client
                .post("https://api.cohere.com/v2/embed")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let vec: Vec<f32> = resp["embeddings"]["float"][0]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("missing embedding in Cohere response"))?
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            Ok(vec)
        })
    }
}

// --- OpenAI Provider ---

pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "text-embedding-3-small".to_string(),
            client: reqwest::Client::new(),
        }
    }
}

impl EmbeddingProvider for OpenAiProvider {
    fn dimensions(&self) -> usize {
        1536
    }

    fn embed_batch(&self, texts: &[String]) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
        let texts = texts.to_vec();
        Box::pin(async move {
            if texts.is_empty() {
                return Ok(vec![]);
            }
            let body = serde_json::json!({
                "model": self.model,
                "input": texts,
            });
            let resp: serde_json::Value = self.client
                .post("https://api.openai.com/v1/embeddings")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let data = resp["data"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("missing data in OpenAI response"))?;

            let mut result = Vec::with_capacity(data.len());
            for item in data {
                let vec: Vec<f32> = item["embedding"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("missing embedding in OpenAI response"))?
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect();
                result.push(vec);
            }
            Ok(result)
        })
    }
}

// --- Helpers ---

pub fn to_pgvector(v: &[f32]) -> pgvector::Vector {
    pgvector::Vector::from(v.to_vec())
}

pub fn create_provider(cfg: &crate::config::EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    match cfg.provider.as_str() {
        "cohere" => Ok(Box::new(CohereProvider::new(cfg.api_key.clone()))),
        "openai" => Ok(Box::new(OpenAiProvider::new(cfg.api_key.clone()))),
        other => bail!("unknown embedding provider: {other}"),
    }
}
