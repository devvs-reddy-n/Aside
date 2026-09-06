//! Cloud embeddings over plain `reqwest` (`status.md` 7.1(c) / 7.4) —
//! mirrors `llm/openai.rs`'s house style exactly (no SDK crate, a pure
//! request-body-builder function kept separate from the HTTP call so it's
//! testable without a network round trip), simpler in one respect: OpenAI's
//! embeddings endpoint is a plain JSON request/response, no SSE stream to
//! hand-roll.
//!
//! **Confidence note, same shape as `llm/openai.rs`'s:** `POST
//! /v1/embeddings` with a `Bearer` key and a `{"model","input"}` body,
//! returning `{"data":[{"embedding":[...]}]}`, has been OpenAI's stable,
//! documented embeddings shape for a long time — used here with high
//! confidence. The exact current default model name and its output
//! dimensionality (`DEFAULT_MODEL`/`DEFAULT_DIMENSIONS` below) were written
//! from general knowledge, not verified against live docs this session
//! (same honest gap `llm/openai.rs` states for its own `DEFAULT_MODEL`) —
//! both are one-line `pub const`s to update. **Never called with a real key
//! this session** — same gap this project already carries for every cloud
//! STT engine (`status.md` 4.10/4.13) and every cloud LLM provider
//! (`status.md` 6.7): the request-building is real and tested against a
//! hand-built fixture shape, the actual HTTP round trip is not.

use serde_json::json;

use super::embed::{Embedder, EmbedderKind};

const ENDPOINT: &str = "https://api.openai.com/v1/embeddings";
/// See this module's doc comment.
pub const DEFAULT_MODEL: &str = "text-embedding-3-small";
/// `text-embedding-3-small`'s documented native output size.
pub const DEFAULT_DIMENSIONS: usize = 1536;

pub struct CloudEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl CloudEmbedder {
    pub fn new(client: reqwest::Client, api_key: String) -> Self {
        Self {
            client,
            api_key,
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

/// The request body — a pure function so it's testable without a network
/// call, matching `llm/openai.rs::build_request_body`'s shape.
pub fn build_request_body(model: &str, text: &str) -> serde_json::Value {
    json!({ "model": model, "input": text })
}

/// Extracts the embedding vector from a parsed response body. `None` on any
/// shape mismatch (missing/malformed `data[0].embedding`) rather than
/// panicking — an API error response or a future schema change should
/// surface as a caught `Err` upstream, not a crash here.
pub fn extract_embedding(response: &serde_json::Value) -> Option<Vec<f32>> {
    response["data"][0]["embedding"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_f64())
            .map(|v| v as f32)
            .collect()
    })
}

#[async_trait::async_trait]
impl Embedder for CloudEmbedder {
    fn kind(&self) -> EmbedderKind {
        EmbedderKind::Cloud
    }

    fn dimensions(&self) -> usize {
        DEFAULT_DIMENSIONS
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let body = build_request_body(&self.model, text);
        let response = self
            .client
            .post(ENDPOINT)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("embeddings request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("embeddings API returned {status}: {text}"));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("failed to parse embeddings response: {e}"))?;
        extract_embedding(&json)
            .ok_or_else(|| "embeddings response missing data[0].embedding".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_carries_model_and_input() {
        let body = build_request_body("text-embedding-3-small", "hello world");
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"], "hello world");
    }

    #[test]
    fn extracts_embedding_from_a_realistic_fixture() {
        let fixture = json!({
            "object": "list",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.1, 0.2, 0.3] }],
            "model": "text-embedding-3-small",
        });
        let vec = extract_embedding(&fixture).expect("should extract");
        assert_eq!(vec, vec![0.1f32, 0.2, 0.3]);
    }

    #[test]
    fn missing_data_field_returns_none_not_panic() {
        let fixture = json!({ "error": { "message": "invalid api key" } });
        assert!(extract_embedding(&fixture).is_none());
    }

    #[test]
    fn empty_data_array_returns_none() {
        let fixture = json!({ "data": [] });
        assert!(extract_embedding(&fixture).is_none());
    }

    #[test]
    fn kind_and_dimensions_report_correctly() {
        let e = CloudEmbedder::new(reqwest::Client::new(), "sk-fake".to_string());
        assert_eq!(e.kind(), EmbedderKind::Cloud);
        assert_eq!(e.dimensions(), DEFAULT_DIMENSIONS);
    }
}
