//! RAG + whole-session context awareness (`status.md` Phase 7).
//!
//! As a live call runs long, a suggested-answer request (Phase 6's
//! `llm::maybe_trigger`) only ever sees the one transcript line that
//! triggered it — not anything said earlier in the same conversation. This
//! phase adds the ability to search back through everything said so far and
//! pull in the most relevant earlier chunks before asking the AI (e.g. a
//! follow-up about visa status ten minutes after it was first discussed).
//!
//! **Flagged performance-sensitive in `status.md` itself** — the owner
//! specifically asked for a real benchmark before picking an approach, not
//! a guess (`status.md` 7.1). That benchmark's real numbers, methodology,
//! and the resulting decision are in `docs/evidence.md` / `docs/decisions.md`
//! — short version: a pure-Rust hashed term-frequency vectorizer
//! ([`embed_tfidf`]) is the default (near-zero CPU, no model to load, always
//! available), with a local MiniLM ONNX embedder ([`embed_onnx`]) and a
//! cloud embeddings API ([`embed_cloud`]) built as real, switchable
//! alternatives behind the same [`embed::Embedder`] trait.
//!
//! ## Module map
//! - [`chunker`] — splits transcript text into ~80-word chunks, streaming
//!   (`status.md` 7.3/7.7).
//! - [`embed`] — the `Embedder` trait + `EmbedderKind` (`status.md` 7.4).
//! - [`embed_tfidf`] / [`embed_onnx`] / [`embed_onnx_cache`] / [`embed_cloud`]
//!   — the three real implementations.
//! - [`retriever`] — top-k cosine similarity + score threshold (`status.md`
//!   7.5). Deliberately **not** using `sqlite-vec` — see that module's own
//!   doc comment and `docs/decisions.md` for why (tested, confirmed
//!   compatible with this project's `rusqlite`, but not adopted: plain-Rust
//!   cosine similarity is already fast enough at this project's real scale
//!   — see the perf regression test in `retriever`'s and `indexer`'s test
//!   modules and `docs/evidence.md`'s benchmark numbers).
//! - [`indexer`] — incremental indexing wired into `pipeline.rs`'s
//!   `emit_all` (`status.md` 7.7).
//!
//! Fact extraction ([`crate::facts::extract`]) and its storage
//! (`storage::facts`) are Phase 7 work too (`status.md` 7.8/7.9) but live
//! outside this module, matching `status.md`'s own file layout
//! (`facts/extract.rs`, not `rag/facts/extract.rs`).

pub mod chunker;
pub mod embed;
pub mod embed_cloud;
pub mod embed_onnx;
pub mod embed_onnx_cache;
pub mod embed_tfidf;
pub mod hybrid;
pub mod indexer;
pub mod retriever;

/// `storage::settings` key holding the active embedder kind — mirrors
/// `stt::commands`'s `ENGINE_SETTING_KEY` / `llm::PROVIDER_SETTING_KEY`
/// shape. Unset means "use the default" (`EmbedderKind::TfIdf`), not an
/// error — see `indexer::active_embedder`.
pub const EMBEDDER_SETTING_KEY: &str = "rag_embedder";

/// `storage::settings` key holding the OpenAI embeddings API key, mirroring
/// `llm::LlmProviderKind::api_key_setting`'s per-provider key shape (kept
/// separate from any Phase 6 LLM provider key — a saved chat-answer key and
/// a saved embeddings key are two different credentials even when they
/// happen to be for the same provider).
pub const CLOUD_API_KEY_SETTING: &str = "rag_embed_api_key_openai";
