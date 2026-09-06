//! Google Gemini streaming, over plain `reqwest` + hand-rolled SSE — no
//! `google-generative-ai`/`genai` SDK crate.
//!
//! **Confidence note:** this is the one provider where this phase's
//! research (`docs/evidence.md`, "AI answer suggestions research") found
//! the *exact* wire shape actually sitting in a reference repo's own code,
//! rather than relying purely on general knowledge: OpenCluely
//! (`src/services/llm.service.js`'s `_streamRequestForModel`) hand-rolls
//! the identical thing — endpoint
//! `https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse`,
//! `x-goog-api-key` header, response lines split on `\n`, `data:`-prefixed
//! lines JSON-parsed, text read from `candidates[0].content.parts[].text`.
//! That's a real, verified reference for the streaming shape, so this is
//! higher confidence than a guess — but it is still a *different app's*
//! code, on a Google API that moves faster than OpenAI's/Anthropic's, seen
//! at a point in time. The exact current default *model name*
//! (`DEFAULT_MODEL` below) is not independently verified this session.

use futures_util::StreamExt;
use serde_json::json;

use super::sse::SseDecoder;
use super::{prompt, AnswerSink, LlmProvider, LlmProviderKind};

/// See this module's doc comment — model string not independently verified
/// this session.
pub const DEFAULT_MODEL: &str = "gemini-2.0-flash";

/// Ready-made choices offered in Settings. Same "not a closed list, one
/// line to update, not independently verified" caveat as `DEFAULT_MODEL`
/// above — see `openai::MODEL_PRESETS` for the full reasoning.
pub const MODEL_PRESETS: &[&str] = &[
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
    "gemini-1.5-pro",
];

/// Default model for the cheap, asynchronous background memory-compression
/// pipeline (`llm::background`) — the "flash-lite" entry of
/// [`MODEL_PRESETS`] above, not the default answer model.
pub const BACKGROUND_MODEL: &str = "gemini-2.0-flash-lite";

fn endpoint(model: &str) -> String {
    format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse")
}

pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiProvider {
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

/// Pure — testable without a network call. Gemini's `systemInstruction` is
/// a top-level field, separate from `contents` (which carries only the
/// user turn here — no conversation history in this phase, see
/// `prompt.rs`'s doc comment).
pub fn build_request_body(question: &str, context: Option<&str>) -> serde_json::Value {
    let (system, user) = prompt::compose_system_and_user(question, context);
    json!({
        "systemInstruction": {
            "parts": [{"text": system}],
        },
        "contents": [
            {"role": "user", "parts": [{"text": user}]},
        ],
    })
}

/// Extracts the text delta from one SSE `data:` payload. Gemini has no
/// `[DONE]` sentinel — the stream simply ends (the outer byte-stream
/// closing is the only stop signal, unlike OpenAI/Groq's literal
/// `[DONE]` or Anthropic's named `message_stop` event).
pub fn extract_delta(data: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(str::to_string)
}

#[async_trait::async_trait]
impl LlmProvider for GeminiProvider {
    fn kind(&self) -> LlmProviderKind {
        LlmProviderKind::Gemini
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
        let resp = self
            .client
            .post(endpoint(&self.model))
            .header("x-goog-api-key", &self.api_key)
            .json(&build_request_body(question, context))
            .send()
            .await
            .map_err(|e| format!("gemini: request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("gemini: request rejected ({status}): {body}"));
        }

        let mut decoder = SseDecoder::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("gemini: stream read error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            for event in decoder.push(&text) {
                if let Some(delta) = extract_delta(&event.data) {
                    if !delta.is_empty() {
                        sink.on_delta(&delta);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_uses_system_instruction_and_contents_not_openais_messages_shape() {
        let body = build_request_body("what is rust", None);
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            prompt::SYSTEM_PROMPT
        );
        assert_eq!(body["contents"][0]["role"], "user");
        assert!(body["contents"][0]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("what is rust"));
        assert!(
            body.get("messages").is_none(),
            "must not accidentally use OpenAI's message shape"
        );
    }

    #[test]
    fn request_body_splices_retrieved_context_into_the_user_part() {
        let body = build_request_body("what is rust", Some("[them] we use Rust for the backend"));
        let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
        assert!(text.contains("[them] we use Rust for the backend"));
    }

    #[test]
    fn extract_delta_reads_the_first_candidates_part_text() {
        let data = r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"}}]}"#;
        assert_eq!(extract_delta(data), Some("Hello".to_string()));
    }

    #[test]
    fn extract_delta_returns_none_for_malformed_json() {
        assert_eq!(extract_delta("not json"), None);
    }

    #[test]
    fn extract_delta_returns_none_when_candidates_is_empty() {
        assert_eq!(extract_delta(r#"{"candidates":[]}"#), None);
    }

    #[test]
    fn extract_delta_returns_none_for_a_finish_only_chunk_with_no_parts() {
        // A trailing chunk can carry only finishReason/usage metadata.
        let data = r#"{"candidates":[{"content":{"role":"model"},"finishReason":"STOP"}]}"#;
        assert_eq!(extract_delta(data), None);
    }

    #[test]
    fn endpoint_includes_the_model_and_sse_query_param() {
        let url = endpoint("gemini-2.0-flash");
        assert!(url.contains("/models/gemini-2.0-flash:streamGenerateContent"));
        assert!(url.contains("alt=sse"));
    }

    #[test]
    fn provider_kind_is_gemini() {
        let p = GeminiProvider::new(reqwest::Client::new(), "goog-test".to_string(), None);
        assert_eq!(p.kind(), LlmProviderKind::Gemini);
    }

    // Real network path untested this session — no API key available; see
    // `openai.rs`'s test module doc comment for the same honest note.
}
