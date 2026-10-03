//! OpenAI Chat Completions streaming, over plain `reqwest` + hand-rolled
//! SSE — no `openai` SDK crate, matching `stt/deepgram_live.rs`'s "plain
//! HTTP" convention.
//!
//! **Confidence note (per this phase's research step):** the Chat
//! Completions streaming shape used here — `POST /v1/chat/completions`,
//! `Authorization: Bearer`, `stream: true`, SSE lines of
//! `data: {"choices":[{"delta":{"content":"..."}}]}` terminated by a literal
//! `data: [DONE]` — has been stable and documented for a long time and is
//! used with high confidence. Default model is `gpt-5.6-luna` (fastest
//! verified GPT-5.6 Chat Completions id). Invalid ids fail HTTP 400.

use futures_util::StreamExt;
use serde_json::json;

use super::sse::SseDecoder;
use super::timing::TokenUsage;
use super::{prompt, AnswerSink, LlmProvider, LlmProviderKind};

const ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
/// Fastest GPT-5.6 Chat Completions id. The stream shape is still
/// `choices[0].delta.content`. Invalid ids fail with HTTP 400 and surface
/// as `answer-error` plus the canned stand-in reply.
pub const DEFAULT_MODEL: &str = "gpt-5.6-luna";

/// Ready-made choices offered in Ask and Settings, for picking the **main**
/// answer model. Owner's explicit, deliberately short list (2026-08-29) —
/// Sol, 5.1, plain 5, and 5-mini used to be listed here too but the owner
/// asked for exactly these five and no more. **Still not a closed list** —
/// `set_llm_model` accepts any valid-looking name typed into the "or type a
/// model name" field, so a model released after this was written (or Sol/
/// 5.1/5/5-mini, if the owner wants one back) doesn't need a code change,
/// just isn't offered as a one-click preset. Unverified mini ids
/// (`gpt-5.1-mini`, `gpt-5.5-mini`, `gpt-5.6-mini`) were never listed.
pub const MODEL_PRESETS: &[&str] = &[
    "gpt-5.6-luna",
    "gpt-5.6-terra",
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
];

/// Default model for the cheap, asynchronous background memory-compression
/// pipeline (`llm::background`, `window_memory::background`) — deliberately
/// a `-mini` id, and one already verified/listed in [`MODEL_PRESETS`]
/// above, not one of the "unverified, not listed" mini ids that comment
/// calls out.
pub const BACKGROUND_MODEL: &str = "gpt-5.4-mini";

/// The model Screenshot always uses (2026-08-30, owner request: "use GPT
/// Terra as default irrespective what model using in the session") —
/// deliberately ignores whatever text-answer model is configured globally
/// or per-Copilot. A real, previously-reported "screenshot doesn't work at
/// all" symptom is consistent with this: before this pin, a screenshot's
/// model followed the session/Copilot's saved choice, and not every model
/// in [`MODEL_PRESETS`] is necessarily vision-capable — a non-vision model
/// silently makes every screenshot request fail at the provider. Pinning to
/// one known-good vision model removes that whole failure class.
pub const VISION_MODEL: &str = "gpt-5.6-terra";

/// Model families that do hidden "thinking" before answering, and therefore
/// accept a `reasoning_effort` knob. Matched by prefix so a dated variant
/// (`o4-mini-2025-…`) is covered without listing every one.
const REASONING_FAMILIES: &[&str] = &["gpt-5", "o1", "o3", "o4"];

/// Whether `model` is one of the reasoning families above.
pub fn is_reasoning_model(model: &str) -> bool {
    let m = model.trim().to_lowercase();
    REASONING_FAMILIES.iter().any(|f| m.starts_with(f))
}

/// The `reasoning_effort` value to send, or `None` to omit the parameter
/// entirely.
///
/// Hidden thinking is not streamed, so a high effort looks like a frozen
/// screen. GPT-5.1+ and GPT-5.4/5.5/5.6 accept `"none"` and that is the
/// live interview setting. Original `gpt-5` / `gpt-5-mini` and the o-series
/// do not accept `"none"` (or `"minimal"` on newer 5.1+ models) — those
/// keep `"low"`. Ordinary chat models omit the field.
///
/// Paid priority / flex service tiers are not sent. They stay optional and
/// off unless a later setting opts in.
pub fn reasoning_effort_for(model: &str) -> Option<&'static str> {
    if !is_reasoning_model(model) {
        return None;
    }
    let m = model.trim().to_lowercase();
    if m.starts_with("gpt-5.6")
        || m.starts_with("gpt-5.5")
        || m.starts_with("gpt-5.4")
        || m.starts_with("gpt-5.2")
        || m.starts_with("gpt-5.1")
    {
        Some("none")
    } else {
        Some("low")
    }
}

/// Whether `temperature`/`top_p`/`seed` may be sent for `model`. Reasoning
/// models return HTTP 400 for a non-default value of any of them.
///
/// Nothing sends sampling parameters today; this function plus its test
/// exist so that a future "let's add temperature" change can't silently
/// break every reasoning model. It is a tripwire, not dead weight.
///
/// `#[allow(dead_code)]` is honest here: production code doesn't call this
/// *because* nothing currently sends sampling parameters, which is exactly
/// the state it guards. Its test asserts the request body stays free of
/// them.
#[allow(dead_code)]
pub fn supports_sampling_params(model: &str) -> bool {
    !is_reasoning_model(model)
}

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    service_tier: Option<String>,
}

impl OpenAiProvider {
    /// `model: None` falls back to [`DEFAULT_MODEL`] — the behavior every
    /// caller had before the model became selectable.
    pub fn new(client: reqwest::Client, api_key: String, model: Option<String>) -> Self {
        Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            service_tier: None,
        }
    }

    /// Paid flex/priority tiers stay off unless the owner opts in.
    pub fn with_service_tier(mut self, tier: Option<String>) -> Self {
        self.service_tier = normalize_service_tier(tier.as_deref());
        self
    }

    /// Which model this provider will actually use. Lets a test assert the
    /// saved setting really reached the provider, rather than trusting the
    /// plumbing in between — `#[allow(dead_code)]` because that assertion is
    /// its only caller (the trait's `model_name` is what production uses),
    /// and it is the thing that would catch the model setting silently not
    /// arriving.
    #[allow(dead_code)]
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// The request body — a pure function so it's testable without a network
/// call.
pub fn build_request_body(model: &str, question: &str, context: Option<&str>) -> serde_json::Value {
    let (system, user) = prompt::compose_system_and_user(question, context);
    let mut body = json!({
        "model": model,
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "prompt_cache_key": prompt::cache_key(&system),
    });
    if let Some(effort) = reasoning_effort_for(model) {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

/// `"flex"` / `"priority"` only. Anything else is treated as off so a
/// typo cannot silently raise the bill.
pub fn normalize_service_tier(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim).map(str::to_ascii_lowercase) {
        Some(tier) if tier == "flex" || tier == "priority" => Some(tier),
        _ => None,
    }
}

/// Live OpenAI extras. `include_usage` does not change the streamed text.
/// `service_tier` is omitted unless the owner opted in.
pub fn apply_live_openai_options(body: &mut serde_json::Value, service_tier: Option<&str>) {
    body["stream_options"] = json!({ "include_usage": true });
    if let Some(tier) = normalize_service_tier(service_tier) {
        body["service_tier"] = json!(tier);
    }
}

/// Usage object from a final SSE payload. None when the chunk has no usage.
pub fn extract_usage(data: &str) -> Option<TokenUsage> {
    if data.trim() == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let usage = value.get("usage")?;
    if usage.is_null() {
        return None;
    }
    let has_any = usage.get("prompt_tokens").is_some() || usage.get("completion_tokens").is_some();
    if !has_any {
        return None;
    }
    Some(TokenUsage {
        input_tokens: usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        cached_input_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        reasoning_tokens: usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        output_tokens: usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
    })
}

/// Extracts the text delta from one SSE `data:` payload, or `None` for the
/// `[DONE]` sentinel / a chunk with no content (e.g. the first chunk, which
/// only carries the role). Shared by `groq.rs`, which is wire-compatible.
pub fn extract_delta(data: &str) -> Option<String> {
    if data.trim() == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    value["choices"][0]["delta"]["content"]
        .as_str()
        .map(str::to_string)
}

/// The actual HTTP + SSE streaming loop — shared by `openai.rs` and
/// `groq.rs` (wire-compatible APIs), which is why it takes `endpoint`
/// rather than being hardcoded to OpenAI's URL.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_openai_compatible_stream(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    provider_label: &str,
    question: &str,
    context: Option<&str>,
    sink: &mut dyn AnswerSink,
) -> Result<(), String> {
    let body = build_request_body(model, question, context);
    stream_prebuilt_body(client, endpoint, api_key, provider_label, body, sink).await
}

/// The HTTP + SSE half, with the request body handed in already built.
///
/// Split out from [`run_openai_compatible_stream`] so the vision path
/// ([`stream_vision_answer`]) reuses this exact decoding loop rather than
/// duplicating it — two body builders, one stream reader. The alternative
/// (copying the loop) is precisely how the two paths would drift on error
/// handling or `[DONE]` framing later.
async fn stream_prebuilt_body(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    provider_label: &str,
    body: serde_json::Value,
    sink: &mut dyn AnswerSink,
) -> Result<(), String> {
    sink.on_request_dispatch();
    let resp = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("{provider_label}: request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "{provider_label}: request rejected ({status}): {body}"
        ));
    }

    sink.on_http_headers();

    let mut decoder = SseDecoder::new();
    let mut stream = resp.bytes_stream();
    let mut saw_sse = false;
    let mut saw_text = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("{provider_label}: stream read error: {e}"))?;
        let text = String::from_utf8_lossy(&chunk);
        for event in decoder.push(&text) {
            if !saw_sse {
                saw_sse = true;
                sink.on_sse_event();
            }
            if event.data.trim() == "[DONE]" {
                return if saw_text { Ok(()) } else { Err(format!("{provider_label}: the AI returned no readable answer. Please try again.")) };
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.data) {
                if let Some(error) = value.get("error") {
                    return Err(format!("{provider_label}: {}", error.get("message").and_then(|m| m.as_str()).unwrap_or("The AI stream failed.")));
                }
            }
            if let Some(delta) = extract_delta(&event.data) {
                if !delta.is_empty() {
                    saw_text |= !delta.trim().is_empty();
                    sink.on_model_text(&delta);
                    sink.on_delta(&delta);
                }
            } else if let Some(usage) = extract_usage(&event.data) {
                sink.on_usage(usage);
            }
        }
    }
    if saw_text { Ok(()) } else { Err(format!("{provider_label}: the AI stream ended without an answer. Please try again.")) }
}

// --- Vision (⌘⇧↩ "look at my screen", 2026-08-25) ---
//
// A parallel function rather than a widened `LlmProvider` trait, and
// deliberately so: `LlmProviderKind::ENABLED` is `[OpenAi]` only, so
// widening the trait would force three dormant provider files to grow a
// "vision not supported" arm for no benefit, and `FallbackLlmProvider` has
// no meaningful vision fallback to orchestrate anyway. Stated cost: a
// vision failure has no automatic second provider to try. Everything
// downstream — `AnswerSink`, `SplittingSink`, the `answer-*` events, the
// panel's `AnswerCard` — is reused completely unchanged.

/// The vision request body — pure, so its shape is testable without a
/// network call or a real screenshot.
///
/// Content-array ordering is **text first, image second**, matching OpenAI's
/// own documented examples. `detail: "high"` because the screen usually
/// holds small text (code, a document) that the model has to actually read;
/// the low-detail mode downsamples aggressively.
pub fn build_vision_request_body(
    model: &str,
    jpeg_base64: &str,
    context: Option<&str>,
    question: Option<&str>,
) -> serde_json::Value {
    let mut body = json!({
        "model": model,
        "stream": true,
        "messages": [
            {"role": "system", "content": super::vision_prompt::VISION_SYSTEM_PROMPT},
            {"role": "user", "content": [
                {"type": "text", "text": super::vision_prompt::vision_user_prompt(context, question)},
                {"type": "image_url", "image_url": {
                    "url": format!("data:image/jpeg;base64,{jpeg_base64}"),
                    "detail": "high",
                }},
            ]},
        ],
        "prompt_cache_key": prompt::cache_key(super::vision_prompt::VISION_SYSTEM_PROMPT),
    });
    if let Some(effort) = reasoning_effort_for(model) {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

/// Streams an answer about a screenshot, through the same sink (and so the
/// same events, and so the same panel UI) as an ordinary spoken question.
pub async fn stream_vision_answer(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    jpeg_base64: &str,
    context: Option<&str>,
    question: Option<&str>,
    sink: &mut dyn AnswerSink,
) -> Result<(), String> {
    let body = build_vision_request_body(model, jpeg_base64, context, question);
    stream_prebuilt_body(client, ENDPOINT, api_key, "openai", body, sink).await
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    fn kind(&self) -> LlmProviderKind {
        LlmProviderKind::OpenAi
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
        let mut body = build_request_body(&self.model, question, context);
        apply_live_openai_options(&mut body, self.service_tier.as_deref());
        stream_prebuilt_body(&self.client, ENDPOINT, &self.api_key, "openai", body, sink).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn vision_http_stream_surfaces_errors_and_empty_responses() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        #[derive(Default)]
        struct TextSink(String);
        impl AnswerSink for TextSink {
            fn on_delta(&mut self, text: &str) { self.0.push_str(text); }
            fn on_done(&mut self, _: bool) {}
            fn on_error(&mut self, _: &str) {}
        }
        for (events, expected_error) in [
            ("data: {\"choices\":[{\"delta\":{\"content\":\"Screen answer\"}}]}\n\ndata: [DONE]\n\n", None),
            ("data: [DONE]\n\n", Some("no readable answer")),
            ("data: {\"error\":{\"message\":\"Vision model unavailable\"}}\n\n", Some("Vision model unavailable")),
            ("", Some("ended without an answer")),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&request[..header_end]);
                        let length: usize = header.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|value| value.trim().parse().unwrap())).unwrap();
                        if request.len() >= header_end + 4 + length {
                            let body: serde_json::Value = serde_json::from_slice(&request[header_end + 4..header_end + 4 + length]).unwrap();
                            assert_eq!(body["messages"][1]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,test-image");
                            break;
                        }
                    }
                }
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", events.len(), events).unwrap();
            });
            let mut sink = TextSink::default();
            let body = build_vision_request_body(VISION_MODEL, "test-image", None, None);
            let result = stream_prebuilt_body(&reqwest::Client::new(), &format!("http://{address}"), "test-key", "test-provider", body, &mut sink).await;
            server.join().unwrap();
            if let Some(message) = expected_error { assert!(result.unwrap_err().contains(message)); }
            else { assert!(result.is_ok()); assert_eq!(sink.0, "Screen answer"); }
        }
    }

    #[test]
    fn request_body_has_stream_true_and_both_messages() {
        let body = build_request_body("gpt-4o-mini", "what is rust", None);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert!(body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("what is rust"));
    }

    #[test]
    fn a_chat_model_gets_no_reasoning_effort_parameter_at_all() {
        // Sending it to a non-reasoning model is an error, not a no-op —
        // omitted entirely, not set to some "off" value.
        let body = build_request_body("gpt-4o-mini", "q", None);
        assert!(body.get("reasoning_effort").is_none(), "got {body}");
    }

    #[test]
    fn gpt_5_6_live_interview_models_send_reasoning_effort_none() {
        for model in [
            "gpt-5.6-luna",
            "gpt-5.6-terra",
            "gpt-5.6-sol",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.1",
        ] {
            let body = build_request_body(model, "q", None);
            assert_eq!(
                body["reasoning_effort"], "none",
                "model {model} produced {body}"
            );
        }
    }

    #[test]
    fn original_gpt_5_and_o_series_keep_low_because_none_is_unsupported() {
        for model in ["gpt-5", "gpt-5-mini", "o1-preview", "o3-mini", "o4-mini"] {
            let body = build_request_body(model, "q", None);
            assert_eq!(
                body["reasoning_effort"], "low",
                "model {model} produced {body}"
            );
        }
    }

    #[test]
    fn luna_and_terra_are_offered_presets_and_the_request_names_them() {
        assert!(MODEL_PRESETS.contains(&"gpt-5.6-luna"));
        assert!(MODEL_PRESETS.contains(&"gpt-5.6-terra"));
        assert_eq!(MODEL_PRESETS[0], DEFAULT_MODEL);
        let body = build_request_body("gpt-5.6-terra", "q", None);
        assert_eq!(body["model"], "gpt-5.6-terra");
        assert_eq!(body["stream"], true);
        assert_eq!(body["reasoning_effort"], "none");
    }

    #[test]
    fn reasoning_effort_is_never_minimal() {
        // `minimal` was removed after the original gpt-5 line and now
        // returns HTTP 400 on newer models — a hard failure of the whole
        // answer mid-interview, not a slower one. This test is the guard
        // against someone "optimizing" `low` down to `minimal`.
        for model in [
            "gpt-5",
            "gpt-5.6-luna",
            "o1",
            "o3",
            "o4-mini",
            "gpt-4o-mini",
        ] {
            assert_ne!(
                reasoning_effort_for(model),
                Some("minimal"),
                "model {model}"
            );
        }
        assert_ne!(reasoning_effort_for("gpt-5.6-luna"), Some("low"));
    }

    #[test]
    fn reasoning_families_are_matched_by_prefix_so_dated_variants_are_covered() {
        assert!(is_reasoning_model("o4-mini-2025-04-16"));
        assert!(is_reasoning_model("gpt-5.1-turbo"));
        assert!(!is_reasoning_model("gpt-4o"));
        assert!(!is_reasoning_model("gpt-4.1-mini"));
    }

    #[test]
    fn no_sampling_parameters_are_ever_sent_for_a_reasoning_model() {
        // Nothing sends these today; this is a tripwire so a future
        // "let's add temperature" change can't silently 400 every
        // reasoning model.
        for model in ["gpt-5", "o3-mini"] {
            assert!(!supports_sampling_params(model));
            let body = build_request_body(model, "q", None);
            for param in ["temperature", "top_p", "seed"] {
                assert!(
                    body.get(param).is_none(),
                    "{model} body must omit {param}: {body}"
                );
            }
        }
        assert!(supports_sampling_params("gpt-4o-mini"));
    }

    #[test]
    fn the_request_carries_a_stable_prompt_cache_key() {
        let a = build_request_body("gpt-4o-mini", "first question", None);
        let b = build_request_body("gpt-4o-mini", "a totally different question", None);
        assert_eq!(
            a["prompt_cache_key"], b["prompt_cache_key"],
            "the key must key off the stable system prefix, not the question"
        );
        assert!(a["prompt_cache_key"]
            .as_str()
            .unwrap()
            .starts_with("aside-"));
    }

    #[test]
    fn encoded_context_keeps_messages_zero_byte_identical_across_questions() {
        let sys = format!(
            "{}\n\nFact ledger:\n- visa_status: STEM OPT",
            prompt::SYSTEM_PROMPT
        );
        let a = build_request_body(
            "gpt-4o-mini",
            "q1",
            Some(&prompt::encode_messages(&sys, "user one")),
        );
        let b = build_request_body(
            "gpt-4o-mini",
            "q2",
            Some(&prompt::encode_messages(&sys, "user two")),
        );
        assert_eq!(a["messages"][0], b["messages"][0]);
        assert_eq!(a["prompt_cache_key"], b["prompt_cache_key"]);
        assert_ne!(a["messages"][1]["content"], b["messages"][1]["content"]);
        assert!(!a["messages"][0]["content"].as_str().unwrap().contains("q1"));
    }

    #[test]
    fn a_custom_model_reaches_the_provider_instead_of_the_default() {
        let p = OpenAiProvider::new(
            reqwest::Client::new(),
            "sk".to_string(),
            Some("gpt-4o".to_string()),
        );
        assert_eq!(p.model(), "gpt-4o");
    }

    #[test]
    fn no_model_chosen_falls_back_to_the_built_in_default() {
        let p = OpenAiProvider::new(reqwest::Client::new(), "sk".to_string(), None);
        assert_eq!(p.model(), DEFAULT_MODEL);
    }

    #[test]
    fn the_default_model_is_one_of_the_offered_presets() {
        // Otherwise the Settings dropdown would open showing nothing
        // selected while a different model was actually in use.
        assert!(
            MODEL_PRESETS.contains(&DEFAULT_MODEL),
            "presets: {MODEL_PRESETS:?}"
        );
    }

    #[test]
    fn request_body_splices_retrieved_context_into_the_user_message() {
        let body = build_request_body(
            "gpt-4o-mini",
            "what is rust",
            Some("[you] I use Rust daily"),
        );
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("[you] I use Rust daily"));
        assert!(content.contains("what is rust"));
    }

    #[test]
    fn extract_delta_reads_the_content_field() {
        let data = r#"{"id":"x","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        assert_eq!(extract_delta(data), Some("Hello".to_string()));
    }

    #[test]
    fn extract_delta_returns_none_for_the_done_sentinel() {
        assert_eq!(extract_delta("[DONE]"), None);
        assert_eq!(extract_delta(" [DONE] "), None);
    }

    #[test]
    fn extract_delta_returns_none_for_a_role_only_first_chunk() {
        let data = r#"{"choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
        assert_eq!(extract_delta(data), None);
    }

    #[test]
    fn extract_delta_returns_none_for_malformed_json_rather_than_panicking() {
        assert_eq!(extract_delta("not json at all"), None);
    }

    #[test]
    fn extract_delta_returns_none_for_an_empty_choices_array() {
        let data = r#"{"choices":[]}"#;
        assert_eq!(extract_delta(data), None);
    }

    #[test]
    fn provider_kind_is_openai() {
        let p = OpenAiProvider::new(reqwest::Client::new(), "sk-test".to_string(), None);
        assert_eq!(p.kind(), LlmProviderKind::OpenAi);
    }

    #[test]
    fn paid_service_tiers_are_omitted_unless_the_owner_opts_in() {
        let mut body = build_request_body("gpt-5.6-luna", "q", None);
        apply_live_openai_options(&mut body, None);
        assert!(body.get("service_tier").is_none(), "got {body}");
        assert_eq!(body["stream_options"]["include_usage"], true);
        let mut priority = build_request_body("gpt-5.6-luna", "q", None);
        apply_live_openai_options(&mut priority, Some("priority"));
        assert_eq!(priority["service_tier"], "priority");
        let mut clean = build_request_body("gpt-5.6-luna", "q", None);
        apply_live_openai_options(&mut clean, Some("fast"));
        assert!(
            clean.get("service_tier").is_none(),
            "typos must not enable a paid tier: {clean}"
        );
        assert_eq!(
            normalize_service_tier(Some("FLEX")).as_deref(),
            Some("flex")
        );
        assert_eq!(normalize_service_tier(Some("")), None);
    }

    #[test]
    fn extract_usage_reads_cached_and_reasoning_tokens() {
        let data = r#"{
            "choices": [],
            "usage": {
                "prompt_tokens": 1200,
                "completion_tokens": 80,
                "prompt_tokens_details": { "cached_tokens": 900 },
                "completion_tokens_details": { "reasoning_tokens": 0 }
            }
        }"#;
        let usage = extract_usage(data).expect("usage");
        assert_eq!(usage.input_tokens, Some(1200));
        assert_eq!(usage.cached_input_tokens, Some(900));
        assert_eq!(usage.reasoning_tokens, Some(0));
        assert_eq!(usage.output_tokens, Some(80));
        assert!(extract_usage(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#).is_none());
    }

    // NOTE: `run_openai_compatible_stream`'s actual network path (a real
    // 200 SSE stream, or a real 401) is untested here — it needs a live
    // API key, which this session does not have. Same honest gap as
    // Deepgram/Speechmatics (`status.md` 4.10/4.13): the pure request/parse
    // logic above is real, unit-tested code; the HTTP call itself has never
    // been exercised against the real service.
}
