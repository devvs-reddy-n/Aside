//! The [`Embedder`] trait (`status.md` 7.4) — the one swap point behind
//! which every "turn text into a vector" approach lives, so
//! `retriever.rs`'s cosine-similarity code never has to know or care which
//! one produced the numbers it's comparing.
//!
//! Three real implementations, matching `status.md` 7.1's three researched
//! approaches exactly:
//! - [`crate::rag::embed_tfidf::HashedTermFrequencyEmbedder`] — pure Rust,
//!   no model, the measured default (see `docs/decisions.md`).
//! - [`crate::rag::embed_onnx::OnnxEmbedder`] — local MiniLM via `ort`.
//! - [`crate::rag::embed_cloud::CloudEmbedder`] — OpenAI's embeddings API
//!   over plain `reqwest`, mirroring `llm/openai.rs`'s house style.

use serde::{Deserialize, Serialize};

/// Which embedding backend — mirrors `stt::EngineKind` / `llm::LlmProviderKind`'s
/// shape exactly (an enum with `ALL`, a stable on-disk settings string
/// separate from serde's wire casing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedderKind {
    TfIdf,
    Onnx,
    Cloud,
}

impl EmbedderKind {
    pub const ALL: [EmbedderKind; 3] =
        [EmbedderKind::TfIdf, EmbedderKind::Onnx, EmbedderKind::Cloud];

    pub fn as_setting_value(self) -> &'static str {
        match self {
            EmbedderKind::TfIdf => "tfidf",
            EmbedderKind::Onnx => "onnx_minilm",
            EmbedderKind::Cloud => "cloud_openai",
        }
    }

    pub fn from_setting_value(s: &str) -> Option<Self> {
        match s {
            "tfidf" => Some(EmbedderKind::TfIdf),
            "onnx_minilm" => Some(EmbedderKind::Onnx),
            "cloud_openai" => Some(EmbedderKind::Cloud),
            _ => None,
        }
    }
}

/// Turns one piece of text into a fixed-length vector. `async` (rather than
/// a plain sync fn) even though the pure-Rust backend never awaits anything
/// real — it's what lets `OnnxEmbedder` (blocking ONNX Runtime inference,
/// must run via `spawn_blocking` exactly like `stt::parakeet_local`'s
/// `infer_chunk`) and `CloudEmbedder` (a real HTTP call) share one trait
/// with the pure-Rust backend, so `retriever.rs` and the incremental
/// indexer never need to know which kind of embedder they're holding.
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    fn kind(&self) -> EmbedderKind;

    /// The fixed length every vector this embedder produces will have.
    /// Cosine similarity between two vectors from *different* embedders is
    /// meaningless (different dimensions, different spaces) — callers
    /// (`rag::indexer`) must never mix embedders within one session's
    /// stored chunks; see that module's doc comment.
    fn dimensions(&self) -> usize;

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
}
