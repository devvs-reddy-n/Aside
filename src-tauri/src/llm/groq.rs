//! Groq's chat-completions API, which Groq itself documents as
//! OpenAI-compatible (same request shape, same SSE framing, same
//! `Authorization: Bearer` header) — so this reuses `openai.rs`'s pure
//! request-building/delta-extraction/streaming-loop functions rather than
//! duplicating them, and only supplies what actually differs: the base URL
//! and the default model. Still a distinct provider (own file, own
//! `LlmProviderKind` variant, own API key setting), per `status.md` 6.3's
//! "four providers" — this is code reuse for an identical wire format, not
//! a merged provider.
//!
//! **Confidence note:** the OpenAI-compatibility claim itself (Groq's own
//! positioning) is used with high confidence; the exact current default
//! model string is not independently verified this session — same caveat
//! as `openai.rs`'s `DEFAULT_MODEL`.

use super::openai::run_openai_compatible_stream;
use super::{AnswerSink, LlmProvider, LlmProviderKind};

const ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "llama-3.3-70b-versatile";

/// Ready-made choices offered in Settings. Same "not a closed list, one
/// line to update, not independently verified" caveat as `DEFAULT_MODEL`
/// above — see `openai::MODEL_PRESETS` for the full reasoning.
pub const MODEL_PRESETS: &[&str] = &["llama-3.3-70b-versatile", "llama-3.1-8b-instant"];

/// Default model for the cheap, asynchronous background memory-compression
/// pipeline (`llm::background`) — the smaller "8b-instant" entry of
/// [`MODEL_PRESETS`] above, not the default answer model.
pub const BACKGROUND_MODEL: &str = "llama-3.1-8b-instant";

pub struct GroqProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GroqProvider {
    /// `model: None` falls back to [`DEFAULT_MODEL`] — the behavior every
    /// caller had before the model became selectable (2026-08-25).
    pub fn new(client: reqwest::Client, api_key: String, model: Option<String>) -> Self {
        Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for GroqProvider {
    fn kind(&self) -> LlmProviderKind {
        LlmProviderKind::Groq
    }

    fn model_name(&self) -> Option<String> {
        Some(self.model.clone())
    }

    async fn stream_answer(
        &self,
        question: &str,
        context: Option<&str>,
        sink: &mut dyn AnswerSink,
    ) -> Result<(), String> {
        run_openai_compatible_stream(
            &self.client,
            ENDPOINT,
            &self.api_key,
            &self.model,
            "groq",
            question,
            context,
            sink,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_is_groq() {
        let p = GroqProvider::new(reqwest::Client::new(), "gk-test".to_string(), None);
        assert_eq!(p.kind(), LlmProviderKind::Groq);
    }

    #[test]
    fn endpoint_is_groqs_own_domain_not_openais() {
        // A copy/paste mistake here would silently send Groq-keyed requests
        // to OpenAI's servers, or vice versa — cheap to pin.
        assert!(ENDPOINT.starts_with("https://api.groq.com/"));
    }

    // Real request/SSE parsing behavior is already covered by
    // `openai.rs`'s tests against the exact functions this module reuses
    // (`build_request_body`/`extract_delta`/`run_openai_compatible_stream`).
    // No real API key was used this session — same honest gap as every
    // other cloud provider in this file set.
}
