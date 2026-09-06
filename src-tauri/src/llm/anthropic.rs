//! Anthropic Messages API streaming, over plain `reqwest` + hand-rolled
//! SSE — no `@anthropic-ai`/`anthropic` SDK crate.
//!
//! **Confidence note:** the streaming envelope — named SSE events
//! (`event: content_block_delta`, `event: message_stop`, ...), each
//! carrying `data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}` —
//! has been stable and documented for a long time and is used with high
//! confidence, including the header shape (`x-api-key`, not
//! `Authorization`; `anthropic-version`, a required header). The exact
//! current default *model name* (`DEFAULT_MODEL` below) is not
//! independently verified this session — written from general knowledge,
//! not checked against Anthropic's live docs (no network access to verify
//! during this session), and Anthropic's model lineup has moved on
//! multiple times since. It's a `pub const`, one line to update.

use futures_util::StreamExt;
use serde_json::json;

use super::sse::SseDecoder;
use super::{prompt, AnswerSink, LlmProvider, LlmProviderKind};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// See this module's doc comment — envelope shape well-established, exact
/// model string not independently verified this session.
pub const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20241022";

/// Ready-made choices offered in Settings. Same "not a closed list, one
/// line to update, not independently verified" caveat as `DEFAULT_MODEL`
/// above — see `openai::MODEL_PRESETS` for the full reasoning.
pub const MODEL_PRESETS: &[&str] = &["claude-3-5-sonnet-20241022", "claude-3-5-haiku-20241022"];

/// Default model for the cheap, asynchronous background memory-compression
/// pipeline (`llm::background`) — the cheaper "haiku" entry of
/// [`MODEL_PRESETS`] above, not the default answer model.
pub const BACKGROUND_MODEL: &str = "claude-3-5-haiku-20241022";
const MAX_TOKENS: u32 = 1024;

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
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

/// Pure — testable without a network call.
pub fn build_request_body(model: &str, question: &str, context: Option<&str>) -> serde_json::Value {
    let (system, user) = prompt::compose_system_and_user(question, context);
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": system,
        "messages": [
            {"role": "user", "content": user},
        ],
    })
}

/// Extracts a text delta from one named SSE event. Only
/// `content_block_delta` events with a `text_delta` inner type carry
/// answer text — `message_start`/`content_block_start`/`ping`/
/// `message_delta`/`message_stop` all carry metadata this provider doesn't
/// need. `event_name` is `None` for a block with no `event:` line at all,
/// which Anthropic's real stream never sends but which this function still
/// handles safely (returns `None`) rather than assuming it's always `Some`.
pub fn extract_delta(event_name: Option<&str>, data: &str) -> Option<String> {
    if event_name != Some("content_block_delta") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    if value["delta"]["type"] != "text_delta" {
        return None;
    }
    value["delta"]["text"].as_str().map(str::to_string)
}

/// True once the stream has reached its terminal event — a defensive
/// second stop condition alongside the outer byte-stream simply closing,
/// since Anthropic (unlike OpenAI/Groq) has no `[DONE]` sentinel inside
/// `data:` — the stop signal here is the *event name* itself.
fn is_stream_end(event_name: Option<&str>) -> bool {
    event_name == Some("message_stop")
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    fn kind(&self) -> LlmProviderKind {
        LlmProviderKind::Anthropic
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
            .post(ENDPOINT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&build_request_body(&self.model, question, context))
            .send()
            .await
            .map_err(|e| format!("anthropic: request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("anthropic: request rejected ({status}): {body}"));
        }

        let mut decoder = SseDecoder::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("anthropic: stream read error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            for event in decoder.push(&text) {
                let event_name = event.event.as_deref();
                if is_stream_end(event_name) {
                    return Ok(());
                }
                if let Some(delta) = extract_delta(event_name, &event.data) {
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
    fn request_body_uses_system_field_not_a_system_message() {
        // Anthropic's Messages API takes the system prompt as a top-level
        // `system` field, not a `{"role":"system",...}` entry in `messages`
        // (that's OpenAI's shape) — pinning this distinction directly.
        let body = build_request_body("claude-3-5-sonnet-20241022", "hi", None);
        assert_eq!(body["system"], prompt::SYSTEM_PROMPT);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn request_body_splices_retrieved_context_into_the_user_message() {
        let body = build_request_body(
            "claude-3-5-sonnet-20241022",
            "hi",
            Some("earlier: my notice period is two weeks"),
        );
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("my notice period is two weeks"));
    }

    #[test]
    fn extract_delta_reads_text_from_a_content_block_delta_event() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        assert_eq!(
            extract_delta(Some("content_block_delta"), data),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn extract_delta_ignores_non_content_block_delta_events() {
        let start = r#"{"type":"message_start","message":{"id":"msg_1"}}"#;
        assert_eq!(extract_delta(Some("message_start"), start), None);
        let ping = r#"{"type":"ping"}"#;
        assert_eq!(extract_delta(Some("ping"), ping), None);
    }

    #[test]
    fn extract_delta_ignores_a_content_block_delta_that_is_not_text() {
        // Anthropic also streams input_json_delta for tool-use blocks —
        // irrelevant to this provider but must not be mistaken for text.
        let data = r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"}}"#;
        assert_eq!(extract_delta(Some("content_block_delta"), data), None);
    }

    #[test]
    fn extract_delta_with_no_event_name_is_none() {
        assert_eq!(
            extract_delta(None, r#"{"delta":{"type":"text_delta","text":"x"}}"#),
            None
        );
    }

    #[test]
    fn is_stream_end_only_matches_message_stop() {
        assert!(is_stream_end(Some("message_stop")));
        assert!(!is_stream_end(Some("message_start")));
        assert!(!is_stream_end(Some("content_block_delta")));
        assert!(!is_stream_end(None));
    }

    #[test]
    fn provider_kind_is_anthropic() {
        let p = AnthropicProvider::new(reqwest::Client::new(), "sk-ant-test".to_string(), None);
        assert_eq!(p.kind(), LlmProviderKind::Anthropic);
    }

    // Real network path untested this session — no API key available; see
    // `openai.rs`'s test module doc comment for the same honest note.
}
