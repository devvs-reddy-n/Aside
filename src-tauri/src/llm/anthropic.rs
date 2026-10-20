//! Anthropic Messages API streaming, over plain `reqwest` + hand-rolled SSE
//! (Rust has no official Anthropic SDK).
//!
//! Tuned for a live copilot, where every second of silence matters:
//! - **Thinking kept to the minimum each model allows.** Hidden thinking is
//!   not shown, so it reads as a frozen screen. Claude Opus 5.5 cannot turn
//!   thinking off (only `output_config.effort` controls it), so it runs at
//!   `low`. Sonnet 5.5 turns it off with
//!   `thinking: {type: "between_tools"}`; Haiku 5.5 with `disabled`.
//! - **Prompt caching** on the stable system prefix (core, the owner's
//!   rules and guidance, background), so repeat answers in a call read it
//!   at about a tenth of the price and faster.
//! - **Refusal fallback** (`fallbacks: "default"`): if a model's safety
//!   check declines, Anthropic re-runs the request on its recommended
//!   fallback model inside the same stream. Haiku 5.5 has no server-side
//!   fallback, so it is sent without one.

use futures_util::StreamExt;
use serde_json::{json, Value};

use super::sse::SseDecoder;
use super::timing::TokenUsage;
use super::{prompt, AnswerSink, LlmProvider, LlmProviderKind};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Header for the `fallbacks: "default"` form (the array form uses a
/// different date; pairing the wrong one returns a 400).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Claude Opus 5.5, the current Opus and Anthropic's recommended default.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

/// The only Claude models offered and accepted: the current Opus, Sonnet
/// and Haiku (Fable is deliberately not offered).
pub const MODEL_PRESETS: &[&str] = &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

/// A ceiling, not a target: answer length comes from the owner's rules.
/// Generous so a long code answer is never cut off mid-line.
const MAX_TOKENS: u32 = 16_000;

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    /// `anthropic-workspace-id`, required by keys not tied to one workspace.
    workspace: Option<String>,
}

impl AnthropicProvider {
    /// `model: None` uses [`DEFAULT_MODEL`].
    pub fn new(client: reqwest::Client, api_key: String, model: Option<String>) -> Self {
        Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            workspace: None,
        }
    }

    pub fn with_workspace(mut self, workspace: Option<String>) -> Self {
        self.workspace = workspace.map(|id| id.trim().to_string()).filter(|id| !id.is_empty());
        self
    }
}

/// Anthropic's plain error text, rewritten where Aside can say exactly what
/// to do about it.
pub fn explain_error(message: &str) -> String {
    if message.contains("anthropic-workspace-id") && message.contains("valid workspace ID") {
        return "The Claude workspace ID in Settings isn't valid. Copy it from the ID column on the Claude Console's Workspaces page (it starts with wrkspc_).".to_string();
    }
    if message.contains("anthropic-workspace-id") || message.contains("not scoped to a workspace") {
        return "This Claude key isn't tied to a workspace. Either create a new key for one workspace in the Claude Console (API keys page) and paste it into Settings, or add your Workspace ID under the Claude key in Settings.".to_string();
    }
    if message.contains("Workspace `") && message.contains("not found") {
        return "Claude can't find that workspace for this key. Check the Workspace ID in Settings, or that the key's account belongs to that workspace.".to_string();
    }
    message.to_string()
}

/// Lowest-latency settings each current model accepts. Returns the
/// `thinking` value to send (None = omit) and whether the server-side
/// refusal fallback is available.
fn latency_settings(model: &str) -> (Option<Value>, bool) {
    if model.starts_with("claude-sonnet-5-5") {
        (Some(json!({"type": "between_tools"})), true)
    } else if model.starts_with("claude-haiku-5-5") {
        (Some(json!({"type": "disabled"})), false)
    } else {
        // Opus 5.5: thinking is always on; effort is the dial.
        (None, true)
    }
}

/// Shared body for text and screenshot answers. `content` is the user turn.
/// The system message as cache-friendly blocks: the shared prefix (rules,
/// guidance, role, experience) cached for an hour, because interview gaps
/// often exceed the default 5 minutes and the prefix can be ~150k tokens;
/// anything after the split marker (screen instructions) is uncached.
fn system_blocks(system: &str) -> Value {
    let (prefix, rest) = prompt::split_cached(system);
    let mut blocks = vec![json!({"type": "text", "text": prefix, "cache_control": {"type": "ephemeral", "ttl": "1h"}})];
    if let Some(rest) = rest.filter(|r| !r.trim().is_empty()) {
        blocks.push(json!({"type": "text", "text": rest}));
    }
    Value::Array(blocks)
}

/// A text answer's user turn: the append-only conversation memory as its own
/// cached block (5 minutes, after the 1-hour prefix as Anthropic requires),
/// then the raw window and closing instructions.
fn user_blocks(user: &str) -> Value {
    match prompt::split_cached(user) {
        (memory, Some(rest)) if !memory.trim().is_empty() => json!([
            {"type": "text", "text": memory, "cache_control": {"type": "ephemeral"}},
            {"type": "text", "text": rest},
        ]),
        _ => json!(prompt::without_split(user)),
    }
}

fn body_with(model: &str, system: &str, content: Value) -> Value {
    let (thinking, fallback) = latency_settings(model);
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": system_blocks(system),
        "messages": [{"role": "user", "content": content}],
        "output_config": {"effort": "low"},
    });
    if let Some(thinking) = thinking {
        body["thinking"] = thinking;
    }
    if fallback {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Pure — testable without a network call.
pub fn build_request_body(model: &str, question: &str, context: Option<&str>) -> Value {
    let (system, user) = prompt::compose_system_and_user(question, context);
    body_with(model, &system, user_blocks(&user))
}

/// Screenshot request: the image first, then the accompanying text.
pub fn build_vision_request_body(model: &str, system: &str, user_text: &str, jpeg_base64: &str) -> Value {
    body_with(
        model,
        system,
        json!([
            {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": jpeg_base64}},
            {"type": "text", "text": prompt::without_split(user_text)},
        ]),
    )
}

/// Writes the call's shared prefix into the cache before the first real
/// answer: the same model, thinking and effort settings as real traffic (so
/// the entry is one real requests hit), `max_tokens: 0` so nothing is
/// generated, and no streaming (required with `max_tokens: 0`).
pub fn build_prewarm_body(model: &str, system: &str) -> Value {
    let mut body = body_with(model, system, json!("Ready."));
    body["max_tokens"] = json!(0);
    body["stream"] = json!(false);
    body
}

/// Text from one named SSE event: only `text_delta`s carry answer text.
/// Thinking blocks, fallback markers and metadata events are skipped.
pub fn extract_delta(event_name: Option<&str>, data: &str) -> Option<String> {
    if event_name != Some("content_block_delta") {
        return None;
    }
    let value: Value = serde_json::from_str(data).ok()?;
    if value["delta"]["type"] != "text_delta" {
        return None;
    }
    value["delta"]["text"].as_str().map(str::to_string)
}

/// Running state across one stream: token counts and the final stop reason.
#[derive(Default)]
struct StreamState {
    usage: TokenUsage,
    stop_reason: Option<String>,
}

impl StreamState {
    /// Reads usage and stop reason from `message_start` / `message_delta`.
    /// Returns an error message for an `error` event.
    fn observe(&mut self, event_name: Option<&str>, data: &str) -> Result<(), String> {
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        match event_name {
            Some("error") => Err(value["error"]["message"]
                .as_str()
                .unwrap_or("The Claude stream failed.")
                .to_string()),
            Some("message_start") => {
                let usage = &value["message"]["usage"];
                let fresh = usage["input_tokens"].as_u64().unwrap_or(0);
                let read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                let written = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                self.usage.input_tokens = Some((fresh + read + written) as u32);
                self.usage.cached_input_tokens = Some(read as u32);
                Ok(())
            }
            Some("message_delta") => {
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_string());
                }
                if let Some(out) = value["usage"]["output_tokens"].as_u64() {
                    self.usage.output_tokens = Some(out as u32);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl AnthropicProvider {
    fn request(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .post(ENDPOINT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION);
        if body.get("fallbacks").is_some() {
            request = request.header("anthropic-beta", FALLBACK_BETA);
        }
        if let Some(workspace) = &self.workspace {
            request = request.header("anthropic-workspace-id", workspace);
        }
        request
    }

    async fn stream_body(&self, body: Value, sink: &mut dyn AnswerSink) -> Result<(), String> {
        let request = self.request(&body);
        sink.on_request_dispatch();
        let resp = request
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("anthropic: request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                .unwrap_or(body);
            return Err(format!("anthropic: request rejected ({status}): {}", explain_error(&message)));
        }
        sink.on_http_headers();

        let mut decoder = SseDecoder::new();
        let mut stream = resp.bytes_stream();
        let mut state = StreamState::default();
        let mut saw_sse = false;
        let mut saw_text = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("anthropic: stream read error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            for event in decoder.push(&text) {
                let event_name = event.event.as_deref();
                if !saw_sse {
                    saw_sse = true;
                    sink.on_sse_event();
                }
                state
                    .observe(event_name, &event.data)
                    .map_err(|e| format!("anthropic: {e}"))?;
                if let Some(delta) = extract_delta(event_name, &event.data) {
                    if !delta.is_empty() {
                        saw_text |= !delta.trim().is_empty();
                        sink.on_model_text(&delta);
                        sink.on_delta(&delta);
                    }
                }
                if event_name == Some("message_stop") {
                    sink.on_usage(state.usage.clone());
                    return finish(saw_text, state.stop_reason.as_deref());
                }
            }
        }
        finish(saw_text, state.stop_reason.as_deref())
    }
}

/// A declined request (after any fallback) or an empty stream is an error,
/// so the chain can try the next provider or show the stand-in reply.
fn finish(saw_text: bool, stop_reason: Option<&str>) -> Result<(), String> {
    if saw_text {
        return Ok(());
    }
    if stop_reason == Some("refusal") {
        return Err("anthropic: Claude declined to answer this request.".to_string());
    }
    Err("anthropic: the AI stream ended without an answer. Please try again.".to_string())
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
        self.stream_body(build_request_body(&self.model, question, context), sink).await
    }

    async fn stream_vision(
        &self,
        system: &str,
        user_text: &str,
        jpeg_base64: &str,
        sink: &mut dyn AnswerSink,
    ) -> Result<(), String> {
        self.stream_body(build_vision_request_body(&self.model, system, user_text, jpeg_base64), sink)
            .await
    }

    async fn prewarm(&self, system: &str) -> Result<TokenUsage, String> {
        let body = build_prewarm_body(&self.model, system);
        let resp = self
            .request(&body)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("anthropic: warm-up failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            let message = value["error"]["message"].as_str().unwrap_or("rejected");
            return Err(format!("anthropic: warm-up rejected ({status}): {}", explain_error(message)));
        }
        let usage = &value["usage"];
        let read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
        let written = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        let fresh = usage["input_tokens"].as_u64().unwrap_or(0);
        Ok(TokenUsage {
            input_tokens: Some((fresh + read + written) as u32),
            cached_input_tokens: Some(read as u32),
            ..TokenUsage::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicitly invoked only: one tiny real request per offered model with
    /// the locally saved Anthropic key (never printed), through the exact
    /// request Aside sends.
    #[tokio::test]
    #[ignore = "real Anthropic account; ASIDE_LIVE_CLAUDE_TEST=1 required"]
    async fn live_claude_probe() {
        assert_eq!(std::env::var("ASIDE_LIVE_CLAUDE_TEST").as_deref(), Ok("1"));
        let path = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.aside.desktop/aside.sqlite3");
        let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let key: String = db.query_row("SELECT value FROM settings WHERE key='llm_api_key_anthropic'", [], |r| r.get(0)).unwrap();
        let workspace: Option<String> = db.query_row("SELECT value FROM settings WHERE key='llm_anthropic_workspace_id'", [], |r| r.get(0)).ok();
        drop(db);
        #[derive(Default)]
        struct Sink(String, Vec<String>);
        impl AnswerSink for Sink {
            fn on_delta(&mut self, t: &str) { self.0.push_str(t); }
            fn on_done(&mut self, _: bool) {}
            fn on_error(&mut self, m: &str) { self.1.push(m.to_string()); }
        }
        let only = std::env::var("ASIDE_CLAUDE_ONLY").ok();
        for model in MODEL_PRESETS {
            if only.as_deref().is_some_and(|o| o != *model) { continue; }
            let provider = AnthropicProvider::new(reqwest::Client::new(), key.trim().to_string(), Some(model.to_string()))
                .with_workspace(workspace.clone());
            let encoded = prompt::encode_messages("You are a test. Reply with exactly: OK", "Say OK.");
            let mut sink = Sink::default();
            let started = std::time::Instant::now();
            let result = provider.stream_answer("Say OK.", Some(&encoded), &mut sink).await;
            let shown = |s: String| s.replace(key.trim(), "[key]");
            println!("LIVE_CLAUDE {model} ms={} result={:?} text={:?}", started.elapsed().as_millis(), result.map_err(shown), sink.0);
            if let Ok(path) = std::env::var("ASIDE_CLAUDE_VISION_JPEG") {
                use base64::Engine;
                let jpeg = base64::engine::general_purpose::STANDARD.encode(std::fs::read(path).unwrap());
                let mut sink = Sink::default();
                let result = provider.stream_vision("Describe images in one short sentence.", "What is in this picture?", &jpeg, &mut sink).await;
                println!("LIVE_CLAUDE_VISION {model} result={:?} text={:?}", result.map_err(|e| e.replace(key.trim(), "[key]")), sink.0);
            }
        }
    }

    /// Explicitly invoked only: three live-call moments (a question, an
    /// introduction, the owner trailing off) through the owner's real rules,
    /// guidance and copilot documents, exactly as a call would send them.
    #[tokio::test]
    #[ignore = "real Anthropic account and owner data; ASIDE_LIVE_MOMENTS_TEST=1 required"]
    async fn live_moment_behaviour() {
        use crate::window_memory::{build_prompt, DefaultContext};
        use crate::window_memory::{ConversationWindow, RawTurn, SessionMemory, Speaker};
        assert_eq!(std::env::var("ASIDE_LIVE_MOMENTS_TEST").as_deref(), Ok("1"));
        let path = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.aside.desktop/aside.sqlite3");
        let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let get = |sql: &str| -> Option<String> { db.query_row(sql, [], |r| r.get(0)).ok() };
        let key = get("SELECT value FROM settings WHERE key='llm_api_key_anthropic'").unwrap();
        let model = get("SELECT value FROM settings WHERE key='llm_model_anthropic'").unwrap_or(DEFAULT_MODEL.into());
        let copilot = std::env::var("ASIDE_COPILOT").unwrap_or("Robert Half".into());
        let row = |col: &str| get(&format!("SELECT {col} FROM copilots WHERE name='{copilot}'"));
        let mut stmt = db.prepare("SELECT d.doc_type, d.file_name, d.raw_text FROM copilots c JOIN copilot_documents cd ON cd.copilot_id=c.id JOIN documents d ON d.id=cd.document_id WHERE c.name=?1 ORDER BY cd.attached_at_ms").unwrap();
        let docs: Vec<(String, String, String)> = stmt.query_map([&copilot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        let mut ctx = DefaultContext {
            instructions: row("instructions"),
            target_role: row("target_role"),
            company: row("company"),
            job_description: row("job_description"),
            // As migration v13 leaves it: the owner's rules after the default.
            rules_override: get("SELECT value FROM settings WHERE key='answer_rules_override'").map(|saved| {
                if saved.contains(crate::window_memory::MY_CONTENT_RULES_HEADING) { saved } else { crate::window_memory::merged_rules(&saved) }
            }),
            ..DefaultContext::default()
        };
        for (kind, name, text) in docs {
            if kind == "resume" && ctx.resume.is_none() { ctx.resume = Some(text); } else { ctx.other_docs.push((name, text)); }
        }
        let turn = |who: &str, text: &str| RawTurn { speaker: if who == "Me" { Speaker::Me } else { Speaker::Other }, text: text.into(), at_ms: 0 };
        let moments = [
            ("question", vec![turn("Other", "Can you walk me through how you handled data quality checks in your pipelines?")]),
            ("introduction", vec![turn("Other", "Hi, I'm Sarah. I lead the platform team here, we're about twelve engineers and we mostly own the streaming ingestion and the internal feature store.")]),
            ("unfinished", vec![
                turn("Other", "Tell me about a challenging project you worked on."),
                turn("Me", "Yeah so at one of my projects we had this issue where the nightly batch was taking way too long and what we did was we started moving the"),
            ]),
            ("why_this_role", vec![turn("Other", "So why are you interested in this role?")]),
        ];
        #[derive(Default)]
        struct Sink(String, Option<TokenUsage>);
        impl AnswerSink for Sink {
            fn on_delta(&mut self, t: &str) { self.0.push_str(t); }
            fn on_done(&mut self, _: bool) {}
            fn on_error(&mut self, _: &str) {}
            fn on_usage(&mut self, usage: TokenUsage) { self.1 = Some(usage); }
        }
        let provider = AnthropicProvider::new(reqwest::Client::new(), key.trim().to_string(), Some(model.clone()));
        let warm_system = build_prompt(&ctx, &SessionMemory::default(), &ConversationWindow { turns: Vec::new(), start_ms: 0, end_ms: 0 }).system;
        let began = std::time::Instant::now();
        let warm = provider.prewarm(&warm_system).await;
        println!("=== PREWARM ms={} prefix_chars={} result={:?}", began.elapsed().as_millis(), warm_system.len(), warm);
        for (label, turns) in moments {
            let window = ConversationWindow { turns, start_ms: 0, end_ms: 0 };
            let prompt = build_prompt(&ctx, &SessionMemory::default(), &window);
            let encoded = prompt::encode_messages(&prompt.system, &prompt.user);
            let mut sink = Sink::default();
            let started = std::time::Instant::now();
            let result = provider.stream_answer("", Some(&encoded), &mut sink).await;
            let usage = sink.1.clone().unwrap_or_default();
            println!("=== MOMENT {label} model={model} ms={} prefix_chars={} input_tokens={:?} cached_tokens={:?} words={} ok={:?}\n{}\n", started.elapsed().as_millis(), prompt.system.len(), usage.input_tokens, usage.cached_input_tokens, sink.0.split_whitespace().count(), result.as_ref().err(), sink.0);
        }
    }

    #[test]
    fn the_prefix_is_cached_for_an_hour_and_memory_gets_its_own_cache_point() {
        let user = format!("CONVERSATION SO FAR\n- earlier{}NEW RAW CONVERSATION WINDOW\nOther: Why?", prompt::CACHE_SPLIT);
        let body = build_request_body("claude-sonnet-5-5", "", Some(&prompt::encode_messages("SHARED PREFIX", &user)));
        assert_eq!(body["system"][0]["text"], "SHARED PREFIX");
        assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["text"], "CONVERSATION SO FAR\n- earlier");
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert!(content[0]["cache_control"].get("ttl").is_none(), "5-minute entry after the 1-hour one");
        assert!(content[1]["text"].as_str().unwrap().contains("Other: Why?"));
        assert!(!body.to_string().contains("ASIDE_CACHE_SPLIT"), "the marker never reaches the model");
    }

    #[test]
    fn a_screenshot_reuses_the_exact_cached_prefix_of_text_answers() {
        let prefix = "HOW THIS REQUEST IS ORGANIZED\n...rules, guidance, experience...\n";
        let text = build_request_body("claude-opus-5-5", "", Some(&prompt::encode_messages(prefix, "Other: Hi")));
        let screen = build_vision_request_body("claude-opus-5-5", &super::super::vision_prompt::vision_system_prompt(prefix), "Look", "abc");
        assert_eq!(text["system"][0], screen["system"][0], "same cached block, byte for byte");
        assert!(screen["system"][1]["text"].as_str().unwrap().contains("SCREEN ANSWERS"));
        assert!(screen["system"][1].get("cache_control").is_none());
    }

    #[test]
    fn the_warm_up_matches_real_traffic_but_generates_nothing() {
        let warm = build_prewarm_body("claude-sonnet-5-5", "SHARED PREFIX");
        let real = build_request_body("claude-sonnet-5-5", "", Some(&prompt::encode_messages("SHARED PREFIX", "Other: Hi")));
        assert_eq!(warm["max_tokens"], 0);
        assert_eq!(warm["stream"], false);
        for field in ["system", "model", "thinking", "output_config", "fallbacks"] {
            assert_eq!(warm[field], real[field], "{field} must match so real answers hit the warmed entry");
        }
    }

    #[test]
    fn only_the_current_claude_models_are_offered_and_opus_5_5_is_the_default() {
        assert_eq!(MODEL_PRESETS, &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]);
        assert!(!MODEL_PRESETS.iter().any(|m| m.contains("fable")), "Fable is not offered");
        assert_eq!(MODEL_PRESETS[0], DEFAULT_MODEL);
    }

    #[test]
    fn the_system_prefix_is_a_cached_block_and_the_question_is_the_user_turn() {
        let encoded = prompt::encode_messages("STABLE PREFIX", "the window");
        let body = build_request_body("claude-opus-5-5", "q", Some(&encoded));
        assert_eq!(body["system"][0]["text"], "STABLE PREFIX");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "the window");
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        assert_eq!(body["stream"], true);
        // No sampling parameters: current models reject non-default values.
        for param in ["temperature", "top_p", "top_k"] {
            assert!(body.get(param).is_none());
        }
    }

    #[test]
    fn each_model_gets_its_lowest_latency_thinking_setting() {
        for model in MODEL_PRESETS {
            let body = build_request_body(model, "q", None);
            assert_eq!(body["output_config"]["effort"], "low", "{model}");
            assert!(body["thinking"].get("budget_tokens").is_none(), "{model}: budgets are rejected");
        }
        // Opus 5.5 rejects any explicit thinking-off value.
        assert!(build_request_body("claude-opus-5-5", "q", None).get("thinking").is_none());
        assert_eq!(build_request_body("claude-sonnet-5-5", "q", None)["thinking"]["type"], "between_tools");
        assert_eq!(build_request_body("claude-haiku-5-5", "q", None)["thinking"]["type"], "disabled");
    }

    #[test]
    fn refusal_fallback_is_on_except_for_haiku_which_has_none() {
        for model in ["claude-opus-5-5", "claude-sonnet-5-5"] {
            assert_eq!(build_request_body(model, "q", None)["fallbacks"], "default", "{model}");
        }
        assert!(build_request_body("claude-haiku-5-5", "q", None).get("fallbacks").is_none());
    }

    #[test]
    fn a_screenshot_sends_the_image_before_the_text() {
        let body = build_vision_request_body("claude-sonnet-5-5", "SYSTEM", "What is on screen?", "abc");
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["media_type"], "image/jpeg");
        assert_eq!(content[0]["source"]["data"], "abc");
        assert_eq!(content[1]["text"], "What is on screen?");
        assert_eq!(body["system"][0]["text"], "SYSTEM");
        assert_eq!(body["model"], "claude-sonnet-5-5");
    }

    #[test]
    fn only_text_deltas_become_answer_text() {
        let text = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        assert_eq!(extract_delta(Some("content_block_delta"), text), Some("Hello".to_string()));
        let thinking = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}"#;
        assert_eq!(extract_delta(Some("content_block_delta"), thinking), None);
        let fallback = r#"{"type":"content_block_start","index":0,"content_block":{"type":"fallback"}}"#;
        assert_eq!(extract_delta(Some("content_block_start"), fallback), None);
        assert_eq!(extract_delta(None, text), None);
    }

    #[test]
    fn usage_counts_cached_tokens_and_a_refusal_with_no_text_is_an_error() {
        let mut state = StreamState::default();
        state
            .observe(
                Some("message_start"),
                r#"{"type":"message_start","message":{"usage":{"input_tokens":40,"cache_read_input_tokens":8000,"cache_creation_input_tokens":0}}}"#,
            )
            .unwrap();
        state
            .observe(Some("message_delta"), r#"{"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":3}}"#)
            .unwrap();
        assert_eq!(state.usage.input_tokens, Some(8040));
        assert_eq!(state.usage.cached_input_tokens, Some(8000));
        assert_eq!(state.usage.output_tokens, Some(3));
        assert!(finish(false, state.stop_reason.as_deref()).unwrap_err().contains("declined"));
        assert!(finish(true, Some("refusal")).is_ok(), "text already shown stays");
        assert!(finish(false, Some("end_turn")).unwrap_err().contains("without an answer"));
        let err = StreamState::default()
            .observe(Some("error"), r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
            .unwrap_err();
        assert_eq!(err, "Overloaded");
    }

    #[test]
    fn workspace_errors_become_plain_instructions() {
        let missing = "This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header with the ID of the workspace to use.";
        assert!(explain_error(missing).contains("Workspace ID"));
        let identity = "anthropic-workspace-id is required when authenticating with an identity-linked API key; send the id of the workspace this request acts in.";
        assert!(explain_error(identity).contains("isn't tied to a workspace"));
        assert!(explain_error("anthropic-workspace-id header must be a valid workspace ID.").contains("isn't valid"));
        assert!(explain_error("Workspace `wrkspc_x` not found.").contains("can't find that workspace"));
        assert_eq!(explain_error("Overloaded"), "Overloaded");
    }

    #[test]
    fn a_blank_workspace_is_not_sent() {
        let p = AnthropicProvider::new(reqwest::Client::new(), "k".into(), None).with_workspace(Some("  ".into()));
        assert!(p.workspace.is_none());
        let p = AnthropicProvider::new(reqwest::Client::new(), "k".into(), None).with_workspace(Some(" wrkspc_abc ".into()));
        assert_eq!(p.workspace.as_deref(), Some("wrkspc_abc"));
    }

    #[test]
    fn provider_kind_and_default_model() {
        let p = AnthropicProvider::new(reqwest::Client::new(), "sk-ant-test".to_string(), None);
        assert_eq!(p.kind(), LlmProviderKind::Anthropic);
        assert_eq!(p.model_name().as_deref(), Some(DEFAULT_MODEL));
    }
}
