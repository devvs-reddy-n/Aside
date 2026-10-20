//! AI answer suggestions (`status.md` Phase 6).
//!
//! When a question lands on the "them" (system-audio) channel, this module
//! asks a cloud LLM provider for a suggested answer and streams it to the
//! panel UI word-by-word — the same way ChatGPT streams its replies. One
//! provider is "active" (chosen in settings, mirroring `stt::EngineKind`'s
//! one-active-engine shape), with the rest available as an automatic
//! fallback chain (`fallback::FallbackLlmProvider`) so a single provider
//! outage doesn't leave the panel silent.
//!
//! ## Research before code (`status.md` 6.1)
//!
//! Both reference apps were read for real before any of this was designed —
//! full findings in `docs/evidence.md`, "AI answer suggestions research".
//! Two decisions this module makes were shaped directly by that reading:
//!
//! - **Trigger on a *final* transcript that looks like a question**, not a
//!   VAD boundary and not an LLM-based intent classifier. Natively's own
//!   code (`electron/main.ts`'s `scheduleAutoAnswer`) explicitly rejects
//!   triggering on VAD `speech_ended` — STT finals race VAD unpredictably —
//!   and reaches for a final transcript instead, exactly what
//!   `pipeline.rs`'s `emit_all` already has in hand. Natively also runs a
//!   regex fast-path (`hasQuestionSignal`) before ever reaching its heavier
//!   local zero-shot ONNX classifier; `trigger::looks_like_a_question`
//!   is that fast-path alone, deliberately without the ONNX layer —
//!   proportionate to a personal project's Phase 6, not a rebuild of
//!   Natively's `IntentClassifier.ts`. OpenCluely instead just coalesces
//!   speech on a silence timer and lets the LLM's own system prompt decide
//!   relevance — rejected here because Aside would then pay for (and stream
//!   a UI update for) an LLM call on every utterance, question or not.
//! - **Provider-level fallback.** The selected provider (OpenAI or
//!   Anthropic) is tried first, then the other one if it has a saved key,
//!   then the canned stand-in reply — a fixed order plus a failure counter
//!   (`fallback::FallbackChain`).
//!
//! ## Module shape
//!
//! - [`LlmProvider`] — the trait, mirroring `stt::SttEngine`'s "plain trait,
//!   async methods" shape. `openai.rs` (GPT-5.6 Sol/Terra/Luna) and
//!   `anthropic.rs` (latest Claude models) implement it over plain `reqwest`
//!   + hand-rolled SSE parsing (`sse.rs`); Rust has no official SDK for
//!   either.
//! - [`fallback::FallbackLlmProvider`] — tries providers in order, falling
//!   back to [`canned_fallback`] if every one fails.
//! - [`trigger`] — the pure "does this look like a question" + cooldown /
//!   in-flight logic, called from `pipeline.rs`.
//! - [`prompt`] — the single-question prompt shared by every provider, now
//!   with an optional retrieved-context block (`retrieve_context`, wired
//!   from `respond_to_question` — `status.md` Phase 7's "pull relevant
//!   earlier chunks into the AI's prompt" payoff).

pub mod anthropic;
pub mod background;
pub mod setup_summary;
pub mod canned_fallback;
pub mod fallback;
pub mod openai;
pub mod prompt;
pub mod question_split;
pub mod sse;
pub mod timing;
pub mod trigger;
pub mod vision_prompt;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

#[cfg(test)]
use crate::rag::retriever::top_k;
use crate::storage::{settings, Db};
use fallback::FallbackLlmProvider;

/// How many retrieved chunks to splice into the prompt, and the minimum
/// cosine score to bother including one at all. `RETRIEVAL_K` is
/// deliberately small (a single triggering question, not a multi-turn chat
/// needing a large context window) — loosely inspired by Natively's
/// `RAGRetriever.ts` `topK`/token-budget idea, scaled down for this
/// project's one-question prompt rather than porting its token-counting
/// budget logic wholesale.
///
/// `RETRIEVAL_THRESHOLD` was measured, not guessed, against the default
/// embedder (`rag::embed_tfidf`) — an adversarial review of this wiring
/// found the first-drafted value (0.05) let a real false positive through:
/// two genuinely unrelated sentences sharing only common grammar words
/// ("your") scored 0.14, higher than a genuine on-topic pair's 0.13 in the
/// same measurement, meaning *no* threshold value could have separated
/// them while stopwords still counted toward the score. Root-caused and
/// fixed in `embed_tfidf::STOPWORDS` (see that constant's doc comment for
/// the full before/after numbers and `docs/evidence.md` for the real
/// measurement) rather than patched over here with a higher threshold.
/// With stopwords filtered, unrelated sentences measured exactly `0.0` and
/// genuine on-topic pairs measured 0.18-0.63 — `0.1` sits with real margin
/// on both sides of that gap.
#[cfg(test)]
const RETRIEVAL_K: usize = 4;
#[cfg(test)]
const RETRIEVAL_THRESHOLD: f32 = 0.1;

/// Which LLM provider — mirrors `stt::EngineKind`'s shape exactly (an enum
/// with `ALL`, a settings-key mapping, and a stable on-disk string separate
/// from serde's wire casing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderKind {
    OpenAi,
    Anthropic,
}

impl LlmProviderKind {
    /// The two providers Aside supports, in the order Settings lists them.
    pub const ALL: [LlmProviderKind; 2] = [LlmProviderKind::OpenAi, LlmProviderKind::Anthropic];

    /// The `storage::settings` key this provider's API key is saved under.
    pub fn api_key_setting(self) -> &'static str {
        match self {
            LlmProviderKind::OpenAi => "llm_api_key_openai",
            LlmProviderKind::Anthropic => "llm_api_key_anthropic",
        }
    }

    /// The `storage::settings` key holding this provider's default model.
    /// Absent means "the provider's first preset".
    pub fn model_setting(self) -> &'static str {
        match self {
            LlmProviderKind::OpenAi => "llm_model_openai",
            LlmProviderKind::Anthropic => "llm_model_anthropic",
        }
    }

    /// The only models offered and accepted for this provider: the latest
    /// generation, nothing older.
    pub fn model_presets(self) -> &'static [&'static str] {
        match self {
            LlmProviderKind::OpenAi => openai::MODEL_PRESETS,
            LlmProviderKind::Anthropic => anthropic::MODEL_PRESETS,
        }
    }

    pub fn supports_model(self, model: &str) -> bool {
        self.model_presets().contains(&model.trim())
    }

    /// The stable string stored in the `settings` table for the selected
    /// provider — separate from serde's wire format, same reasoning as
    /// `EngineKind::as_setting_value`.
    pub fn as_setting_value(self) -> &'static str {
        match self {
            LlmProviderKind::OpenAi => "openai",
            LlmProviderKind::Anthropic => "anthropic",
        }
    }

    pub fn from_setting_value(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(LlmProviderKind::OpenAi),
            "anthropic" => Some(LlmProviderKind::Anthropic),
            _ => None,
        }
    }
}

/// The `storage::settings` key holding the primary (active) provider,
/// mirroring `commands/stt.rs`'s `ENGINE_SETTING_KEY`.
pub const PROVIDER_SETTING_KEY: &str = "llm_provider";
/// The provider chosen with the Settings switch (OpenAI unless Claude is
/// picked). It decides which models every Copilot and call can use.
pub fn selected_provider(conn: &Connection) -> Result<LlmProviderKind, String> {
    Ok(settings::get(conn, PROVIDER_SETTING_KEY)?
        .and_then(|value| LlmProviderKind::from_setting_value(&value))
        .unwrap_or(LlmProviderKind::OpenAi))
}

/// The model `provider` answers with by default: the saved choice while it
/// is still offered, otherwise that provider's first model. Never empty, so
/// switching provider always lands on a real model.
pub fn effective_model(conn: &Connection, provider: LlmProviderKind) -> Result<String, String> {
    Ok(settings::get(conn, provider.model_setting())?
        .filter(|model| provider.supports_model(model))
        .unwrap_or_else(|| provider.model_presets()[0].to_string()))
}

/// Claude workspace ID (`wrkspc_...`), needed only for a Claude key that is
/// not tied to a single workspace. Sent as `anthropic-workspace-id`.
pub const ANTHROPIC_WORKSPACE_SETTING_KEY: &str = "llm_anthropic_workspace_id";

/// Paid OpenAI service tier (`flex` / `priority`). Empty / missing = off.
pub const SERVICE_TIER_SETTING_KEY: &str = "llm_service_tier";
/// Skip a provider on later presses after this many consecutive failures.
pub const CIRCUIT_OPEN_AFTER: u32 = 3;

#[derive(Clone, Debug, Default)]
struct CachedLlmSettings {
    primary: Option<LlmProviderKind>,
    /// Enabled providers that have a key: kind, key, optional model.
    keys: Vec<(LlmProviderKind, String, Option<String>)>,
    service_tier: Option<String>,
    /// Claude workspace for keys not tied to one workspace (`wrkspc_...`).
    anthropic_workspace: Option<String>,
}

/// In-memory provider settings + failure counts. Command+Enter reads this
/// instead of SQLite. Invalidated when a key, model, provider, or tier
/// setting changes.
#[derive(Default)]
pub struct LlmRuntime {
    settings: Mutex<Option<CachedLlmSettings>>,
    failures: Mutex<HashMap<LlmProviderKind, u32>>,
}

impl LlmRuntime {
    pub fn invalidate_settings(&self) {
        *self.settings.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.failures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    pub fn is_open(&self, kind: LlmProviderKind) -> bool {
        self.failures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&kind)
            .copied()
            .unwrap_or(0)
            >= CIRCUIT_OPEN_AFTER
    }

    pub fn record_success(&self, kind: LlmProviderKind) {
        self.failures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(kind, 0);
    }

    pub fn record_failure(&self, kind: LlmProviderKind) {
        let mut guard = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        *guard.entry(kind).or_insert(0) += 1;
    }

    fn settings_or_load(&self, app: &AppHandle) -> Result<CachedLlmSettings, String> {
        {
            let guard = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = guard.as_ref() {
                return Ok(cached.clone());
            }
        }
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        let loaded = load_settings_snapshot(&conn)?;
        drop(conn);
        *self.settings.lock().unwrap_or_else(|p| p.into_inner()) = Some(loaded.clone());
        Ok(loaded)
    }
}

pub fn invalidate_llm_settings(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<LlmRuntime>() {
        runtime.invalidate_settings();
    }
}

/// Where a streamed answer's text chunks go. Kept separate from `AppHandle`
/// on purpose, the same separation `stt::SttEngine` already has from
/// `pipeline.rs`'s event emission: an engine/provider only ever produces
/// data, never touches Tauri directly, so its logic (and
/// `fallback::FallbackLlmProvider`'s orchestration on top of it) is
/// unit-testable with a plain in-memory sink instead of a real running app.
/// `AppHandleSink` below is the one real implementation `respond_to_question`
/// hands to it.
pub trait AnswerSink: Send {
    fn on_delta(&mut self, text: &str);
    fn on_done(&mut self, canned: bool);
    fn on_error(&mut self, message: &str);

    /// Called at most once per question, before the first [`on_delta`] that
    /// belongs to the answer body, with the model's cleaned/paraphrased
    /// restatement of a possibly-disfluent spoken question (2026-08-25,
    /// owner request — see `prompt::SYSTEM_PROMPT`'s doc comment). Empty
    /// when the model didn't produce the expected `Q:`/`A:` format at all —
    /// callers should fall back to displaying the raw question in that
    /// case. Default no-op so every existing implementor (real and test)
    /// keeps compiling unchanged; only [`AppHandleSink`] overrides it for
    /// real. `SplittingSink` below is what actually calls this.
    fn on_question_cleaned(&mut self, _cleaned: &str) {}

    /// Called by [`fallback::FallbackLlmProvider`] immediately before each
    /// provider attempt (and again before its terminal canned-message
    /// fallback) — i.e. every time it's about to call `on_delta` with text
    /// from a *fresh* source. Default no-op; [`SplittingSink`] is the one
    /// real implementor, and needs this to reset its buffered `Q:`/`A:`
    /// parse state between attempts — without it, a provider that fails
    /// partway through streaming (a real, already-accepted edge case per
    /// [`LlmProvider::stream_answer`]'s own doc comment) would otherwise
    /// leave stale buffered text from the failed attempt to corrupt the
    /// next attempt's marker search.
    fn on_attempt_start(&mut self) {}

    /// HTTP response headers received (status already confirmed success).
    fn on_http_headers(&mut self) {}

    /// First SSE frame of this attempt, including role-only chunks.
    fn on_sse_event(&mut self) {}

    /// First raw `delta.content` from the model, before visible-answer parse.
    fn on_model_text(&mut self, _text: &str) {}

    /// Called immediately before the HTTP client dispatches the request.
    fn on_request_dispatch(&mut self) {}

    /// Provider token accounting from the final usage object, if any.
    fn on_usage(&mut self, _usage: timing::TokenUsage) {}
}

/// A pluggable LLM provider. Mirrors `stt::SttEngine`'s "plain trait, async
/// methods, no unnecessary abstraction layers" shape.
///
/// **Streaming contract:** implementations must not call `sink.on_delta`
/// until the HTTP response's status has confirmed success. An
/// auth/network/rate-limit failure is reported as `Err` *before* any delta
/// is emitted, so `FallbackLlmProvider` can move on to the next provider
/// without the panel ever having shown a half-answer from the one that
/// failed. A failure deep inside an already-streaming response (the
/// connection drops mid-reply) is the one case this can't fully protect
/// against — accepted here as a known, rare edge case rather than adding a
/// fourth event type to let the frontend roll back a partial answer.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn kind(&self) -> LlmProviderKind;

    /// Which model this provider will actually use, for the timing readout
    /// and the panel's read-only "answering with…" line. Default `None` so
    /// a test double doesn't have to care; every real provider overrides it.
    fn model_name(&self) -> Option<String> {
        None
    }

    /// Streams a suggested answer for `question`, calling `sink.on_delta`
    /// as text chunks arrive. `context`, when present, is retrieved-chunk
    /// text (`retrieve_context`) spliced into the prompt ahead of the
    /// question — `None` for a session with nothing relevant retrieved yet,
    /// which reproduces the exact prior single-question prompt. Returns
    /// `Ok(())` once the stream completes normally.
    async fn stream_answer(
        &self,
        question: &str,
        context: Option<&str>,
        sink: &mut dyn AnswerSink,
    ) -> Result<(), String>;

    /// Streams an answer about a picture of the screen, with this provider's
    /// own model: `system` is the full system prompt, `user_text` the text
    /// that accompanies the JPEG.
    async fn stream_vision(
        &self,
        system: &str,
        user_text: &str,
        jpeg_base64: &str,
        sink: &mut dyn AnswerSink,
    ) -> Result<(), String> {
        let _ = (system, user_text, jpeg_base64, sink);
        Err(format!("{:?} cannot read screenshots", self.kind()))
    }
}

// --- Events (mirrors pipeline.rs's exact `AppHandle::emit` pattern) ---

#[derive(Debug, Clone, Serialize)]
pub struct AnswerDeltaEvent {
    pub question: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerDoneEvent {
    pub question: String,
    /// True when every configured provider failed and this stream's content
    /// was `canned_fallback::message()`, not a real model reply.
    pub canned: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerErrorEvent {
    pub question: String,
    pub message: String,
}

/// One per question, always emitted (even with an empty `cleaned_question`)
/// so the frontend has an authoritative point to clear a *previous*
/// question's cleaned text — see `SplittingSink::on_question_cleaned`'s
/// call site for why this can't be "only emit when non-empty."
#[derive(Debug, Clone, Serialize)]
pub struct QuestionCleanedEvent {
    pub question: String,
    pub cleaned_question: String,
}

/// Emitted the moment an answer is requested, before any provider work —
/// so the panel can show the question and a working indicator immediately
/// rather than staying blank until the first token arrives.
#[derive(Debug, Clone, Serialize)]
pub struct AnswerStartedEvent {
    pub question: String,
}

pub const EVENT_ANSWER_STARTED: &str = "answer-started";
pub const EVENT_ANSWER_DELTA: &str = "answer-delta";
pub const EVENT_ANSWER_DONE: &str = "answer-done";
pub const EVENT_ANSWER_ERROR: &str = "answer-error";
pub const EVENT_QUESTION_CLEANED: &str = "question-cleaned";

fn emit_delta(app: &AppHandle, question: &str, text: &str) {
    let _ = app.emit(
        EVENT_ANSWER_DELTA,
        AnswerDeltaEvent {
            question: question.to_string(),
            text: text.to_string(),
        },
    );
}

fn emit_done(app: &AppHandle, question: &str, canned: bool) {
    let _ = app.emit(
        EVENT_ANSWER_DONE,
        AnswerDoneEvent {
            question: question.to_string(),
            canned,
        },
    );
}

fn emit_error(app: &AppHandle, question: &str, message: &str) {
    eprintln!("llm answer error for {question:?}: {message}");
    let _ = app.emit(
        EVENT_ANSWER_ERROR,
        AnswerErrorEvent {
            question: question.to_string(),
            message: message.to_string(),
        },
    );
}

fn emit_question_cleaned(app: &AppHandle, question: &str, cleaned_question: &str) {
    let _ = app.emit(
        EVENT_QUESTION_CLEANED,
        QuestionCleanedEvent {
            question: question.to_string(),
            cleaned_question: cleaned_question.to_string(),
        },
    );
}

/// The real, Tauri-backed [`AnswerSink`] — translates the sink calls
/// `fallback::FallbackLlmProvider` (and the providers underneath it) make
/// into the three `answer-*` events on `app`. Everything upstream of this
/// (`FallbackLlmProvider`, every `LlmProvider` impl) never touches
/// `AppHandle` at all, which is what makes them unit-testable without a
/// running Tauri app — see `fallback.rs`'s `CollectingSink` test double.
struct AppHandleSink<'a> {
    app: &'a AppHandle,
    question: &'a str,
}

impl<'a> AppHandleSink<'a> {
    fn new(app: &'a AppHandle, question: &'a str) -> Self {
        Self { app, question }
    }
}

impl<'a> AnswerSink for AppHandleSink<'a> {
    fn on_delta(&mut self, text: &str) {
        emit_delta(self.app, self.question, text);
    }

    fn on_done(&mut self, canned: bool) {
        emit_done(self.app, self.question, canned);
    }

    fn on_error(&mut self, message: &str) {
        emit_error(self.app, self.question, message);
    }

    fn on_question_cleaned(&mut self, cleaned: &str) {
        emit_question_cleaned(self.app, self.question, cleaned);
    }
}

/// Wraps a real [`AnswerSink`] and splits the `Q:`/`A:`-delimited stream
/// every provider now produces (`prompt::SYSTEM_PROMPT`) back into a
/// one-shot [`AnswerSink::on_question_cleaned`] call plus plain answer
/// deltas for everything after the marker — so `FallbackLlmProvider` and
/// every `LlmProvider` impl underneath it stay exactly as they were
/// (they just call `sink.on_delta` with the raw token stream, same as
/// always); this wrapper is the only place that knows the stream has two
/// parts. See `question_split`'s module doc comment for why the parser
/// itself is a separate, pure, chunk-boundary-agnostic module.
struct SplittingSink<'a> {
    inner: &'a mut dyn AnswerSink,
    splitter: question_split::QuestionAnswerSplitter,
}

impl<'a> SplittingSink<'a> {
    fn new(inner: &'a mut dyn AnswerSink) -> Self {
        Self {
            inner,
            splitter: question_split::QuestionAnswerSplitter::default(),
        }
    }
}

impl<'a> AnswerSink for SplittingSink<'a> {
    fn on_delta(&mut self, text: &str) {
        use question_split::SplitEvent;
        match self.splitter.push(text) {
            SplitEvent::Waiting => {}
            SplitEvent::Resolved {
                cleaned_question,
                answer_text,
            } => {
                self.inner.on_question_cleaned(&cleaned_question);
                if !answer_text.is_empty() {
                    self.inner.on_delta(&answer_text);
                }
            }
            SplitEvent::Answer(text) => self.inner.on_delta(&text),
        }
    }

    fn on_done(&mut self, canned: bool) {
        // The marker never arrived at all (a very short reply, or a
        // provider/canned-fallback message that doesn't follow the format)
        // — flush whatever was buffered as plain answer text rather than
        // silently dropping it, and tell the frontend there is no cleaned
        // restatement for this question so it falls back to the raw text.
        if let Some(flushed) = self.splitter.finish() {
            self.inner.on_question_cleaned("");
            self.inner.on_delta(&flushed);
        }
        self.inner.on_done(canned);
    }

    fn on_error(&mut self, message: &str) {
        self.inner.on_error(message);
    }

    fn on_attempt_start(&mut self) {
        // A fresh attempt (or the terminal canned message) is about to send
        // its own, unrelated token stream — any partial `Q:`/`A:` buffer
        // left over from a previous attempt that failed mid-stream must not
        // bleed into this one. See the trait doc comment for the exact
        // failure mode this closes.
        self.splitter = question_split::QuestionAnswerSplitter::default();
    }

    fn on_http_headers(&mut self) {
        self.inner.on_http_headers();
    }

    fn on_sse_event(&mut self) {
        self.inner.on_sse_event();
    }

    fn on_model_text(&mut self, text: &str) {
        self.inner.on_model_text(text);
    }

    fn on_request_dispatch(&mut self) {
        self.inner.on_request_dispatch();
    }

    fn on_usage(&mut self, usage: timing::TokenUsage) {
        self.inner.on_usage(usage);
    }
}

/// Streams a conversation-window answer straight through — the main model's
/// reply is plain visible-answer text only now (2026-08-29 rewrite; see
/// `window_memory::background` for where conversation memory moved to).
/// Kept as its own sink type (rather than using `AppHandleSink` directly)
/// so the final trimmed [`crate::window_memory::ParsedResponse`] is still
/// available to `window_memory::note_press`'s diagnostics after the stream
/// ends.
struct WindowAnswerSink<'a> {
    inner: &'a mut dyn AnswerSink,
    parser: crate::window_memory::StreamParser,
    result: crate::window_memory::ParsedResponse,
}

impl<'a> WindowAnswerSink<'a> {
    fn new(inner: &'a mut dyn AnswerSink) -> Self {
        Self {
            inner,
            parser: crate::window_memory::StreamParser::default(),
            result: crate::window_memory::ParsedResponse::default(),
        }
    }

    fn take_result(self) -> crate::window_memory::ParsedResponse {
        self.result
    }
}

impl<'a> AnswerSink for WindowAnswerSink<'a> {
    fn on_delta(&mut self, text: &str) {
        let visible = self.parser.push(text);
        if !visible.is_empty() {
            self.inner.on_delta(&visible);
        }
    }

    fn on_done(&mut self, canned: bool) {
        let leftover = self.parser.flush();
        if !leftover.is_empty() {
            self.inner.on_delta(&leftover);
        }
        self.result = self.parser.finish();
        self.inner.on_done(canned);
    }

    fn on_error(&mut self, message: &str) {
        self.inner.on_error(message);
    }

    fn on_attempt_start(&mut self) {
        self.parser = crate::window_memory::StreamParser::default();
        self.result = crate::window_memory::ParsedResponse::default();
        self.inner.on_attempt_start();
    }

    fn on_http_headers(&mut self) {
        self.inner.on_http_headers();
    }

    fn on_sse_event(&mut self) {
        self.inner.on_sse_event();
    }

    fn on_model_text(&mut self, text: &str) {
        self.inner.on_model_text(text);
    }

    fn on_request_dispatch(&mut self) {
        self.inner.on_request_dispatch();
    }

    fn on_usage(&mut self, usage: timing::TokenUsage) {
        self.inner.on_usage(usage);
    }
}

// --- Building the fallback chain from settings ---

/// Builds the ordered provider chain from what's actually configured:
/// the primary provider (from `PROVIDER_SETTING_KEY`) first, then the rest
/// of `LlmProviderKind::ALL` in their fixed canonical order — filtered down
/// to only the ones with a saved API key (both the returned chain's
/// `order()` and its actual `providers` reflect this same filtered list,
/// kept in lockstep). A provider with no key would fail instantly on every
/// call, so it's left out rather than included as a guaranteed wasted round
/// trip; if that leaves zero providers, `FallbackLlmProvider` still
/// degrades gracefully straight to `canned_fallback`.
/// The one `reqwest::Client` for the whole process, managed as Tauri state.
///
/// **This replaced a real, measurable latency bug:** `build_provider_chain`
/// used to call `reqwest::Client::new()` itself, and it runs once per
/// question — so every single answer paid a fresh TCP handshake plus a
/// fresh TLS negotiation to the provider before a byte of the actual
/// request moved. A pooled, kept-alive client reuses that connection across
/// the whole session.
///
/// **Deliberately no global `.timeout()`.** `reqwest`'s request timeout
/// covers the *entire* response including the body — on a streaming SSE
/// answer that would cut the stream off mid-sentence at whatever value was
/// set. `connect_timeout` bounds the part that can actually hang without
/// producing anything.
pub struct HttpClient(pub reqwest::Client);

impl HttpClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(300))
            .pool_max_idle_per_host(4)
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            // A builder failure here means the TLS backend didn't
            // initialize, which the plain constructor would hit too — fall
            // back rather than taking the whole app down at startup.
            .unwrap_or_else(|_| reqwest::Client::new());
        Self(client)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Opens a TCP+TLS connection to OpenAI at launch so the first real answer
/// doesn't pay that cost. The response body is thrown away — a 401 without
/// a key is still a successful handshake. Failures are silent: a machine
/// without network must still launch.
pub fn spawn_prewarm(client: reqwest::Client) {
    tauri::async_runtime::spawn(async move {
        let _ = client.get("https://api.openai.com/v1").send().await;
    });
}

fn load_settings_snapshot(conn: &Connection) -> Result<CachedLlmSettings, String> {
    let primary = settings::get(conn, PROVIDER_SETTING_KEY)?
        .and_then(|s| LlmProviderKind::from_setting_value(&s));
    let mut keys = Vec::new();
    for kind in LlmProviderKind::ALL {
        // Normalized here too, so a key pasted with a stray newline, quotes
        // or a "Bearer " prefix still authenticates.
        if let Some(key) = settings::get(conn, kind.api_key_setting())?
            .and_then(|raw| crate::stt::normalize_api_key(&raw).ok())
        {
            let model = settings::get(conn, kind.model_setting())?
                .filter(|model| kind.supports_model(model));
            keys.push((kind, key, model));
        }
    }
    let service_tier = settings::get(conn, SERVICE_TIER_SETTING_KEY)?
        .as_deref()
        .and_then(|s| openai::normalize_service_tier(Some(s)));
    let anthropic_workspace = settings::get(conn, ANTHROPIC_WORKSPACE_SETTING_KEY)?
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
    Ok(CachedLlmSettings {
        primary,
        keys,
        service_tier,
        anthropic_workspace,
    })
}

fn build_provider_chain_from_snapshot(
    snapshot: &CachedLlmSettings,
    client: &reqwest::Client,
) -> FallbackLlmProvider {
    let mut order: Vec<LlmProviderKind> = Vec::new();
    if let Some(p) = snapshot.primary {
        order.push(p);
    }
    for kind in LlmProviderKind::ALL {
        if Some(kind) != snapshot.primary {
            order.push(kind);
        }
    }

    let mut kept_order: Vec<LlmProviderKind> = Vec::new();
    let mut providers: Vec<Box<dyn LlmProvider>> = Vec::new();
    for kind in order {
        let Some((_, key, model)) = snapshot.keys.iter().find(|(k, _, _)| *k == kind) else {
            continue;
        };
        kept_order.push(kind);
        providers.push(build_provider(
            kind,
            client.clone(),
            key.clone(),
            model.clone(),
            snapshot.service_tier.clone(),
            snapshot.anthropic_workspace.clone(),
        ));
    }
    FallbackLlmProvider::new(kept_order, providers)
}

#[cfg(test)]
pub fn build_provider_chain(
    conn: &Connection,
    client: &reqwest::Client,
) -> Result<FallbackLlmProvider, String> {
    let snapshot = load_settings_snapshot(conn)?;
    Ok(build_provider_chain_from_snapshot(&snapshot, client))
}

fn build_provider_chain_for_app(app: &AppHandle) -> Result<FallbackLlmProvider, String> {
    let client = app.state::<HttpClient>().0.clone();
    let runtime = app.state::<LlmRuntime>();
    let mut snapshot = runtime.settings_or_load(app)?;
    // A started Copilot owns the model for the lifetime of that call. The API
    // key still comes from Settings, while the model comes from the immutable
    // call snapshot rather than today's global default.
    if let Some(session_id) = crate::active_session::get(app).and_then(|active| active.id) {
        let db = app.state::<Db>();
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        if let Some((provider, model)) =
            crate::storage::sessions::copilot_model_for_call(&conn, session_id)?
        {
            snapshot.primary = Some(provider);
            if let Some((_, _, saved_model)) = snapshot
                .keys
                .iter_mut()
                .find(|(kind, _, _)| *kind == provider)
            {
                *saved_model = Some(model);
            }
        }
    }
    Ok(build_provider_chain_from_snapshot(&snapshot, &client))
}

fn build_provider(
    kind: LlmProviderKind,
    client: reqwest::Client,
    api_key: String,
    model: Option<String>,
    service_tier: Option<String>,
    anthropic_workspace: Option<String>,
) -> Box<dyn LlmProvider> {
    match kind {
        LlmProviderKind::OpenAi => Box::new(
            openai::OpenAiProvider::new(client, api_key, model).with_service_tier(service_tier),
        ),
        LlmProviderKind::Anthropic => Box::new(
            anthropic::AnthropicProvider::new(client, api_key, model).with_workspace(anthropic_workspace),
        ),
    }
}

// --- Trigger gating: in-flight guard, managed as Tauri state ---

/// Managed as Tauri state (one instance for the app), same shape as
/// `pipeline::TranscriptionRouting`.
#[derive(Default)]
pub struct AnswerTrigger {
    in_flight: AtomicBool,
    last_started_ms: AtomicU64,
    /// Last *user-initiated* answer or screenshot (⌘↩, typed ask, ⌘⇧↩),
    /// used to collapse a local+global shortcut double-fire on the same
    /// keypress into one action.
    last_user_started_ms: AtomicU64,
    /// Bumped on every new answer, including a restart when the final
    /// question changed meaning. A stale stream checks this and goes
    /// silent instead of overwriting the new answer on screen.
    generation: AtomicU64,
}

impl AnswerTrigger {
    pub fn release(&self) {
        self.in_flight.store(false, Ordering::SeqCst);
    }

    /// End Session / a new session: every in-flight stream becomes stale
    /// so it cannot paint into the empty panel.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.in_flight.store(false, Ordering::SeqCst);
    }
}

/// Held for the whole of one answer (spoken, typed, or screenshot).
/// Dropping it is what lets the next question through — including the
/// case where the panel is focused and both the global shortcut and the
/// local key handler fire on the same keypress. The second claim fails
/// and that call is a no-op, which is the whole point.
pub(crate) struct AnswerClaim {
    app: AppHandle,
    pub(crate) generation: u64,
}

impl Drop for AnswerClaim {
    fn drop(&mut self) {
        let state = self.app.state::<AnswerTrigger>();
        // A superseded answer must not release the lock the *new* answer
        // is still holding.
        if state.generation.load(Ordering::SeqCst) == self.generation {
            state.release();
        }
    }
}

/// Start a new answer even if one is already streaming. The old stream
/// keeps running in the background but its events are dropped (see
/// [`GatedSink`]). Used when a partial-question guess was wrong.
pub(crate) fn begin_or_supersede(app: &AppHandle) -> AnswerClaim {
    let state = app.state::<AnswerTrigger>();
    state.in_flight.store(true, Ordering::SeqCst);
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.last_started_ms.store(now_ms(), Ordering::SeqCst);
    AnswerClaim {
        app: app.clone(),
        generation,
    }
}

/// User-initiated (typed ask, ⌘↩, ⌘⇧↩): ignores a second press that lands
/// within [`trigger::USER_ACTION_DEBOUNCE_MS`] of the last user action —
/// the helper-focused local+global shortcut double-fire.
pub(crate) fn try_begin_user_answer(app: &AppHandle) -> Option<AnswerClaim> {
    let state = app.state::<AnswerTrigger>();
    let now = now_ms();
    let last_user = state.last_user_started_ms.load(Ordering::SeqCst);
    if trigger::is_duplicate_user_action(last_user, now) {
        return None;
    }
    state.last_user_started_ms.store(now, Ordering::SeqCst);
    Some(begin_or_supersede(app))
}

/// Called when a session ends or a new one starts, so a stream that was
/// already running cannot keep writing into the next session.
pub(crate) fn invalidate_answers(app: &AppHandle) {
    app.state::<AnswerTrigger>().invalidate();
}

fn generation_is_live(app: &AppHandle, generation: u64) -> bool {
    app.state::<AnswerTrigger>()
        .generation
        .load(Ordering::SeqCst)
        == generation
}

fn emit_answer_started_if_live(app: &AppHandle, generation: u64, question: String) {
    if generation_is_live(app, generation) {
        let _ = app.emit(EVENT_ANSWER_STARTED, AnswerStartedEvent { question });
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pure: ranks `candidates` against `query_vec` with `rag::retriever::top_k`
/// and joins the winning chunks' text, one per line, in score order.
/// `None` for an empty ranking (nothing scored above `threshold`, or no
/// candidates at all) — the exact "no context" signal `prompt::user_prompt`
/// already treats the same as `None` from the start.
#[cfg(test)]
fn build_context_string(
    query_vec: &[f32],
    candidates: &[(Uuid, String, Vec<f32>)],
    k: usize,
    threshold: f32,
) -> Option<String> {
    let by_id: std::collections::HashMap<Uuid, &str> = candidates
        .iter()
        .map(|(id, text, _)| (*id, text.as_str()))
        .collect();
    let for_scoring: Vec<(Uuid, Vec<f32>)> = candidates
        .iter()
        .map(|(id, _, vec)| (*id, vec.clone()))
        .collect();

    let scored = top_k(query_vec, &for_scoring, k, threshold);
    if scored.is_empty() {
        return None;
    }
    let joined = scored
        .iter()
        .filter_map(|s| by_id.get(&s.chunk_id))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Builds the currently-configured provider chain and streams an answer for
/// `question`, emitting `answer-delta`/`answer-done`/`answer-error` along
/// the way. The one real entry point both the automatic trigger
/// (`maybe_trigger`) and the manual test command
/// (`commands::llm::trigger_answer`) call — so a subagent testing "ask a
/// real question" from the Bench exercises the exact same path a live
/// question would. `session_id` is `None` for `PanelWindow.tsx`'s existing
/// ephemeral usage (see `pipeline.rs`'s doc comment) — retrieval is simply
/// skipped in that case, same as Phase 7's indexing already does.
pub async fn respond_to_question(
    app: AppHandle,
    session_id: Option<Uuid>,
    question: String,
    length: Option<crate::window_memory::LengthOverride>,
) {
    let Some(claim) = try_begin_user_answer(&app) else {
        return;
    };
    answer_with_claim(app, session_id, question, claim, length).await;
}

/// Command+Enter conversation window: one request, no RAG, answer streamed
/// first, memory merged after. Claim the generation slot *before* freezing
/// so a double-tap cannot snapshot a window and then drop it. New speech
/// cannot cancel this stream (`auto_answer` stays off on this path).
pub async fn respond_to_window(
    app: AppHandle,
    session_id: Option<Uuid>,
    length: Option<crate::window_memory::LengthOverride>,
) -> Result<(), String> {
    let mut marks = timing::AnswerMarks::start();
    let Some(claim) = try_begin_user_answer(&app) else {
        return Ok(());
    };
    let Some(selected) = crate::window_memory::freeze_for_answer(&app) else {
        drop(claim);
        return Err(crate::window_memory::EMPTY_WINDOW_NOTICE.to_string());
    };
    marks.frozen_at = Some(std::time::Instant::now());
    let frozen = selected.window;
    // Queued right after freeze, before the answer even starts streaming —
    // never awaited here, and never awaited after the answer finishes
    // either. See `window_memory::background`'s doc comment for the
    // single-consumer queue this feeds and the ordering guarantee it gives.
    // A running call can still use its rolling in-memory context. The durable
    // context notes and AI summary shown in History, however, are part of the
    // saved call record and are written only when Save transcript is enabled.
    let history_session_id = crate::active_session::get(&app)
        .filter(|session| session.save_transcript)
        .and_then(|_| session_id);
    if selected.is_new {
        crate::window_memory::enqueue_window(&app, history_session_id, frozen.clone());
    }
    window_answer_with_claim(app, session_id, frozen, claim, marks, length).await;
    Ok(())
}

async fn answer_with_claim(
    app: AppHandle,
    session_id: Option<Uuid>,
    question: String,
    claim: AnswerClaim,
    length: Option<crate::window_memory::LengthOverride>,
) {
    let generation = claim.generation;
    let mut marks = timing::AnswerMarks::start();
    crate::conversation::note_llm_dispatched(&app);
    // Painted before any provider work so the panel shows the question and a
    // "working on it" state immediately, instead of staying blank until the
    // first token lands. Perceived latency only — it does not make the
    // answer arrive sooner, and is labelled as such in `docs/decisions.md`.
    // Generation-gated: a superseded stream must not reset the panel to
    // the old hypothesis after a Restart or End Session.
    emit_answer_started_if_live(&app, generation, question.clone());

    let chain = build_provider_chain_for_app(&app);
    // Same one context mechanism a Cmd+Enter answer uses — default context
    // (rules + resume/job description) plus the growing per-window summary
    // — not the separate, older "recent exchanges" system this path used to
    // read from. A typed Chat question is wrapped as a single synthetic "Me"
    // turn so `window_memory::build_request` treats it exactly like a real
    // spoken window; this is a real behavior fix, not a relabeling — Chat
    // answers previously had no access to the growing conversation summary
    // at all.
    let synthetic_window = crate::window_memory::ConversationWindow {
        turns: vec![crate::window_memory::RawTurn {
            speaker: crate::window_memory::Speaker::Me,
            text: question.clone(),
            at_ms: 0,
        }],
        start_ms: 0,
        end_ms: 0,
    };
    let prompt = crate::window_memory::build_request_with_length(
        &app,
        session_id,
        &synthetic_window,
        length,
    );
    let context = Some(prompt::encode_messages(&prompt.system, &prompt.user));
    marks.context_built_at = Some(std::time::Instant::now());
    marks.body_ready_at = Some(std::time::Instant::now());
    marks.context_chars = context.as_deref().map(str::len).unwrap_or(0);
    let runtime = app.state::<LlmRuntime>();

    match chain {
        Ok(chain) => {
            marks.provider = chain.order().first().map(|k| format!("{k:?}"));
            marks.model = chain.active_model();
            // No Q:/A: splitting: the typed text already is the question,
            // and the answer rules ask for plain text. Splitting held the
            // first 300 characters back and cut any answer containing "A:".
            let mut real_sink = AppHandleSink::new(&app, &question);
            let mut gated = GatedSink::new(&mut real_sink, &app, generation);
            let mut sink = TimingSink::new(&mut gated, &mut marks);
            chain
                .stream_with_gate(&question, context.as_deref(), &mut sink, Some(&runtime))
                .await;
        }
        Err(e) => {
            let mut real_sink = AppHandleSink::new(&app, &question);
            let mut gated = GatedSink::new(&mut real_sink, &app, generation);
            let mut sink = TimingSink::new(&mut gated, &mut marks);
            sink.on_error(&e);
            sink.on_delta(canned_fallback::message());
            sink.on_done(true);
        }
    }

    marks.done_at = Some(std::time::Instant::now());
    let still_live = generation_is_live(&app, generation);
    if still_live {
        copy_answer_marks_into_conversation(&app, &marks);
        let summary_text = marks
            .answer_preview
            .clone()
            .filter(|s| !s.trim().is_empty());
        let summary = timing::summarize(&marks);
        eprintln!("{}", timing::log_line(&summary));
        let _ = app.emit(timing::EVENT_ANSWER_TIMING, summary);
        crate::conversation::note_answer_done(&app, summary_text.clone());
        if let Some(summary) = summary_text {
            crate::conversation::note_ai_suggestion(&app, session_id, &question, &summary);
        }
    }
    drop(claim);
}

async fn window_answer_with_claim(
    app: AppHandle,
    session_id: Option<Uuid>,
    window: crate::window_memory::ConversationWindow,
    claim: AnswerClaim,
    mut marks: timing::AnswerMarks,
    length: Option<crate::window_memory::LengthOverride>,
) {
    let generation = claim.generation;
    crate::conversation::note_llm_dispatched(&app);
    // Name the actual thing being answered. A fixed label made Command+Enter
    // answers look as if they had no question at all.
    let headline = window
        .turns
        .iter()
        .rev()
        .find(|turn| matches!(turn.speaker, crate::window_memory::Speaker::Other))
        .or_else(|| window.turns.last())
        .map(|turn| turn.text.clone())
        .unwrap_or_else(|| "Current conversation".to_string());
    emit_answer_started_if_live(&app, generation, headline.clone());

    let prompt = crate::window_memory::build_request_with_length(&app, session_id, &window, length);
    let encoded = prompt::encode_messages(&prompt.system, &prompt.user);
    marks.context_built_at = Some(std::time::Instant::now());
    marks.body_ready_at = Some(std::time::Instant::now());
    marks.context_chars = encoded.len();

    let chain = build_provider_chain_for_app(&app);
    let runtime = app.state::<LlmRuntime>();

    let parsed = match chain {
        Ok(chain) => {
            marks.provider = chain.order().first().map(|k| format!("{k:?}"));
            marks.model = chain.active_model();
            let mut real_sink = AppHandleSink::new(&app, &headline);
            let mut gated = GatedSink::new(&mut real_sink, &app, generation);
            let mut timed = TimingSink::new(&mut gated, &mut marks);
            let mut sink = WindowAnswerSink::new(&mut timed);
            chain
                .stream_with_gate(&headline, Some(&encoded), &mut sink, Some(&runtime))
                .await;
            sink.take_result()
        }
        Err(e) => {
            let mut real_sink = AppHandleSink::new(&app, &headline);
            let mut gated = GatedSink::new(&mut real_sink, &app, generation);
            let mut timed = TimingSink::new(&mut gated, &mut marks);
            let mut sink = WindowAnswerSink::new(&mut timed);
            sink.on_error(&e);
            sink.on_delta(canned_fallback::message());
            sink.on_done(true);
            sink.take_result()
        }
    };

    marks.done_at = Some(std::time::Instant::now());
    let still_live = generation_is_live(&app, generation);
    if still_live {
        copy_answer_marks_into_conversation(&app, &marks);
        let summary = timing::summarize(&marks);
        eprintln!("{}", timing::log_line(&summary));
        let _ = app.emit(timing::EVENT_ANSWER_TIMING, summary.clone());
        crate::conversation::note_answer_done(&app, Some(parsed.answer.clone()));
        crate::window_memory::note_press(
            &app,
            &window,
            &parsed,
            encoded.len(),
            summary.ttft_ms,
            summary.total_ms,
        );
    }
    drop(claim);
}

fn copy_answer_marks_into_conversation(app: &AppHandle, marks: &timing::AnswerMarks) {
    let state = app.state::<crate::conversation::Conversation>();
    let mut live = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    if live.first_token_at.is_none() {
        live.first_token_at = marks.first_delta_at;
    }
    if live.first_useful_at.is_none() {
        live.first_useful_at = marks.first_useful_at;
    }
}

/// Same one context mechanism every other answer path uses, via
/// `window_memory::build_request`: the stable prefix (core, the owner's
/// strict rules and guidance, background) becomes the screenshot's system
/// message, and the conversation summary plus `question` (the last thing the
/// other person said, if anything, as a synthetic "Other" turn) its context.
fn build_vision_parts(app: &AppHandle, session_id: Option<Uuid>, question: &str) -> (String, String) {
    let turns = if question.trim().is_empty() {
        Vec::new()
    } else {
        vec![crate::window_memory::RawTurn {
            speaker: crate::window_memory::Speaker::Other,
            text: question.to_string(),
            at_ms: 0,
        }]
    };
    let window = crate::window_memory::ConversationWindow {
        turns,
        start_ms: 0,
        end_ms: 0,
    };
    let prompt = crate::window_memory::build_request(app, session_id, &window);
    (vision_prompt::vision_system_prompt(&prompt.system), prompt.user)
}

/// Answers about a picture of the screen (⌘⇧↩). Streams through the exact
/// same `answer-*` events as a spoken question, so the panel renders it with
/// no special-casing.
///
/// No provider fallback here — see `openai::stream_vision_answer`'s
/// surrounding comment for why that is a deliberate trade, not an oversight.
pub async fn respond_to_screenshot(
    app: AppHandle,
    session_id: Option<Uuid>,
    jpeg_base64: String,
    claim: AnswerClaim,
) {
    let generation = claim.generation;
    let mut marks = timing::AnswerMarks::start();
    // The question is whatever the other person last said, if anything — so
    // "help me with what's on screen" lands in the context of the actual
    // conversation rather than cold.
    let spoken = crate::transcript_log::last_them_turn(&app);
    let label = spoken
        .clone()
        .unwrap_or_else(|| "What's on my screen?".to_string());

    emit_answer_started_if_live(&app, generation, label.clone());

    // Same model as the call (a Copilot's own model, or the Settings
    // default): GPT-5.6 and current Claude models all read images.
    let chain = match build_provider_chain_for_app(&app) {
        Ok(chain) if !chain.providers_is_empty() => Ok(chain),
        Ok(_) => Err("No AI key saved yet — open Settings and add one.".to_string()),
        Err(e) => Err(e),
    };

    let mut real_sink = AppHandleSink::new(&app, &label);
    let mut gated = GatedSink::new(&mut real_sink, &app, generation);
    let mut sink = SplittingSink::new(&mut gated);

    let chain = match chain {
        Ok(chain) => chain,
        Err(e) => {
            sink.on_error(&e);
            sink.on_delta(canned_fallback::message());
            sink.on_done(true);
            marks.done_at = Some(std::time::Instant::now());
            if generation_is_live(&app, generation) {
                let summary = timing::summarize(&marks);
                eprintln!("{}", timing::log_line(&summary));
                let _ = app.emit(timing::EVENT_ANSWER_TIMING, summary);
            }
            return;
        }
    };

    let (system, user_context) = build_vision_parts(&app, session_id, spoken.as_deref().unwrap_or(""));
    let user_text = vision_prompt::vision_user_prompt(Some(&user_context), spoken.as_deref());
    marks.context_built_at = Some(std::time::Instant::now());
    marks.context_chars = system.len() + user_text.len();
    marks.provider = chain.order().first().map(|k| format!("{k:?}"));
    marks.model = chain.active_model();

    marks.request_sent_at = Some(std::time::Instant::now());
    {
        let mut timed = TimingSink::new(&mut sink, &mut marks);
        timed.on_attempt_start();
        match chain.stream_vision(&system, &user_text, &jpeg_base64, &mut timed).await {
            Ok(()) => timed.on_done(false),
            Err(e) => {
                timed.on_error(&e);
                timed.on_delta(canned_fallback::message());
                timed.on_done(true);
            }
        }
    }

    marks.done_at = Some(std::time::Instant::now());
    if generation_is_live(&app, generation) {
        let summary = timing::summarize(&marks);
        eprintln!("{}", timing::log_line(&summary));
        let _ = app.emit(timing::EVENT_ANSWER_TIMING, summary);
    }
    drop(claim);
}

/// Drops events from a superseded speculative generation so a stale stream
/// cannot overwrite the answer that replaced it.
struct GatedSink<'a> {
    inner: &'a mut dyn AnswerSink,
    app: &'a AppHandle,
    generation: u64,
}

impl<'a> GatedSink<'a> {
    fn new(inner: &'a mut dyn AnswerSink, app: &'a AppHandle, generation: u64) -> Self {
        Self {
            inner,
            app,
            generation,
        }
    }

    fn is_live(&self) -> bool {
        self.app
            .state::<AnswerTrigger>()
            .generation
            .load(Ordering::SeqCst)
            == self.generation
    }
}

impl<'a> AnswerSink for GatedSink<'a> {
    fn on_delta(&mut self, text: &str) {
        if self.is_live() {
            self.inner.on_delta(text);
        }
    }
    fn on_done(&mut self, canned: bool) {
        if self.is_live() {
            self.inner.on_done(canned);
        }
    }
    fn on_error(&mut self, message: &str) {
        if self.is_live() {
            self.inner.on_error(message);
        }
    }
    fn on_question_cleaned(&mut self, cleaned: &str) {
        if self.is_live() {
            self.inner.on_question_cleaned(cleaned);
        }
    }
    fn on_attempt_start(&mut self) {
        if self.is_live() {
            self.inner.on_attempt_start();
        }
    }
    fn on_http_headers(&mut self) {
        if self.is_live() {
            self.inner.on_http_headers();
        }
    }
    fn on_sse_event(&mut self) {
        if self.is_live() {
            self.inner.on_sse_event();
        }
    }
    fn on_model_text(&mut self, text: &str) {
        if self.is_live() {
            self.inner.on_model_text(text);
        }
    }
    fn on_request_dispatch(&mut self) {
        // Timing must still stamp even if this generation was superseded.
        self.inner.on_request_dispatch();
    }
    fn on_usage(&mut self, usage: timing::TokenUsage) {
        self.inner.on_usage(usage);
    }
}

/// Wraps a sink purely to stamp the first-delta mark. Separate from
/// `SplittingSink` so neither one grows a second responsibility — the same
/// one-job-per-wrapper shape `SplittingSink` itself already follows.
struct TimingSink<'a, 'm> {
    inner: &'a mut dyn AnswerSink,
    marks: &'m mut timing::AnswerMarks,
}

impl<'a, 'm> TimingSink<'a, 'm> {
    fn new(inner: &'a mut dyn AnswerSink, marks: &'m mut timing::AnswerMarks) -> Self {
        Self { inner, marks }
    }
}

impl<'a, 'm> AnswerSink for TimingSink<'a, 'm> {
    fn on_delta(&mut self, text: &str) {
        if self.marks.first_delta_at.is_none() {
            self.marks.first_delta_at = Some(std::time::Instant::now());
        }
        self.marks.answer_chars = self.marks.answer_chars.saturating_add(text.len());
        if self.marks.first_useful_at.is_none()
            && self.marks.answer_chars >= crate::conversation::kpi::USEFUL_ANSWER_CHARS
        {
            self.marks.first_useful_at = Some(std::time::Instant::now());
        }
        if self
            .marks
            .answer_preview
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0)
            < 240
        {
            let mut preview = self.marks.answer_preview.take().unwrap_or_default();
            preview.push_str(text);
            if preview.len() > 240 {
                preview.truncate(240);
            }
            self.marks.answer_preview = Some(preview);
        }
        self.inner.on_delta(text);
    }
    fn on_done(&mut self, canned: bool) {
        self.inner.on_done(canned);
    }
    fn on_error(&mut self, message: &str) {
        self.inner.on_error(message);
    }
    fn on_question_cleaned(&mut self, cleaned: &str) {
        self.inner.on_question_cleaned(cleaned);
    }
    fn on_attempt_start(&mut self) {
        self.inner.on_attempt_start();
    }
    fn on_http_headers(&mut self) {
        if self.marks.headers_at.is_none() {
            self.marks.headers_at = Some(std::time::Instant::now());
        }
        self.inner.on_http_headers();
    }
    fn on_sse_event(&mut self) {
        if self.marks.first_sse_at.is_none() {
            self.marks.first_sse_at = Some(std::time::Instant::now());
        }
        self.inner.on_sse_event();
    }
    fn on_model_text(&mut self, text: &str) {
        if self.marks.first_model_text_at.is_none() {
            self.marks.first_model_text_at = Some(std::time::Instant::now());
        }
        self.inner.on_model_text(text);
    }
    fn on_request_dispatch(&mut self) {
        if self.marks.request_sent_at.is_none() {
            self.marks.request_sent_at = Some(std::time::Instant::now());
        }
        self.inner.on_request_dispatch();
    }
    fn on_usage(&mut self, usage: timing::TokenUsage) {
        self.marks.usage = usage.clone();
        self.inner.on_usage(usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every `AnswerSink` call it receives, in order — the fake
    /// used to test `SplittingSink` without a real `AppHandle`, mirroring
    /// `fallback.rs`'s own `CollectingSink` test double.
    #[derive(Default)]
    struct RecordingSink {
        cleaned_questions: Vec<String>,
        deltas: Vec<String>,
        done: Option<bool>,
        errors: Vec<String>,
    }

    impl AnswerSink for RecordingSink {
        fn on_delta(&mut self, text: &str) {
            self.deltas.push(text.to_string());
        }
        fn on_done(&mut self, canned: bool) {
            self.done = Some(canned);
        }
        fn on_error(&mut self, message: &str) {
            self.errors.push(message.to_string());
        }
        fn on_question_cleaned(&mut self, cleaned: &str) {
            self.cleaned_questions.push(cleaned.to_string());
        }
    }

    #[test]
    fn splitting_sink_extracts_the_cleaned_question_and_forwards_only_the_answer() {
        let mut recording = RecordingSink::default();
        let mut sink = SplittingSink::new(&mut recording);
        sink.on_delta("Q: What is your biggest weakness?\n");
        sink.on_delta("A: I sometimes over-");
        sink.on_delta("prepare.");
        sink.on_done(false);

        assert_eq!(
            recording.cleaned_questions,
            vec!["What is your biggest weakness?"]
        );
        assert_eq!(recording.deltas.join(""), "I sometimes over-prepare.");
        assert_eq!(recording.done, Some(false));
    }

    #[test]
    fn splitting_sink_flushes_unmarked_text_as_the_answer_with_an_empty_cleaned_question() {
        // Exercises the `respond_to_question` error path: `canned_fallback`
        // text never follows the `Q:`/`A:` format at all.
        let mut recording = RecordingSink::default();
        let mut sink = SplittingSink::new(&mut recording);
        sink.on_delta("Sorry, I couldn't reach any AI provider right now.");
        sink.on_done(true);

        assert_eq!(recording.cleaned_questions, vec![""]);
        assert_eq!(
            recording.deltas,
            vec!["Sorry, I couldn't reach any AI provider right now."]
        );
        assert_eq!(recording.done, Some(true));
    }

    #[test]
    fn splitting_sink_resets_on_attempt_start_instead_of_carrying_over_a_failed_attempts_buffer() {
        // Regression test for the exact bug an adversarial review caught:
        // a first attempt streams a partial, unmarked "Q:" restatement,
        // then fails before the `A:` marker ever arrives (a real, already-
        // accepted edge case per `LlmProvider::stream_answer`'s doc
        // comment). Without `on_attempt_start` resetting the splitter, the
        // next attempt's own `Q:`/`A:` text would be appended onto that
        // leftover buffer, corrupting the marker search.
        let mut recording = RecordingSink::default();
        let mut sink = SplittingSink::new(&mut recording);
        sink.on_delta("Q: partial restatement, no answer marker yet");
        sink.on_attempt_start();
        sink.on_delta("Q: a clean second attempt\nA: a clean answer");
        sink.on_done(false);

        assert_eq!(recording.cleaned_questions, vec!["a clean second attempt"]);
        assert_eq!(recording.deltas.join(""), "a clean answer");
    }

    #[test]
    fn splitting_sink_forwards_errors_untouched() {
        let mut recording = RecordingSink::default();
        let mut sink = SplittingSink::new(&mut recording);
        sink.on_error("provider failed");
        assert_eq!(recording.errors, vec!["provider failed"]);
    }

    #[test]
    fn window_answer_sink_streams_plain_text_straight_through() {
        // 2026-08-29 rewrite: the main model's reply is plain visible-answer
        // text only, no embedded JSON or `---ASIDE_META---` trailer — see
        // `window_memory::background` for where conversation memory moved.
        let mut recording = RecordingSink::default();
        let parsed = {
            let mut sink = WindowAnswerSink::new(&mut recording);
            sink.on_delta("Kubernetes is");
            sink.on_delta(" a cluster.");
            sink.on_done(false);
            sink.take_result()
        };
        assert!(
            recording.deltas.join("").contains("Kubernetes is"),
            "first visible words: {:?}",
            recording.deltas
        );
        assert_eq!(parsed.answer, "Kubernetes is a cluster.");
        assert_eq!(recording.done, Some(false));
    }

    #[test]
    fn timing_sink_stamps_first_visible_on_the_first_delta() {
        let mut recording = RecordingSink::default();
        let mut marks = timing::AnswerMarks::start();
        {
            let mut timed = TimingSink::new(&mut recording, &mut marks);
            let mut sink = WindowAnswerSink::new(&mut timed);
            sink.on_delta("Hello");
            sink.on_delta(" there.");
            sink.on_done(false);
        }
        assert!(
            marks.first_delta_at.is_some(),
            "first spoken word must stamp first_delta_at"
        );
        assert!(recording.deltas.join("").contains("Hello"));
    }

    #[test]
    fn llm_runtime_opens_a_circuit_after_repeated_failures_and_clears_on_invalidate() {
        let runtime = LlmRuntime::default();
        assert!(!runtime.is_open(LlmProviderKind::OpenAi));
        for _ in 0..CIRCUIT_OPEN_AFTER {
            runtime.record_failure(LlmProviderKind::OpenAi);
        }
        assert!(runtime.is_open(LlmProviderKind::OpenAi));
        runtime.record_success(LlmProviderKind::OpenAi);
        assert!(!runtime.is_open(LlmProviderKind::OpenAi));
        for _ in 0..CIRCUIT_OPEN_AFTER {
            runtime.record_failure(LlmProviderKind::OpenAi);
        }
        runtime.invalidate_settings();
        assert!(!runtime.is_open(LlmProviderKind::OpenAi));
    }

    #[test]
    fn every_provider_kind_round_trips_through_its_setting_value() {
        for kind in LlmProviderKind::ALL {
            let value = kind.as_setting_value();
            assert_eq!(LlmProviderKind::from_setting_value(value), Some(kind));
        }
    }

    #[test]
    fn an_unrecognized_setting_value_is_none_not_a_panic() {
        assert_eq!(LlmProviderKind::from_setting_value("bogus"), None);
        assert_eq!(LlmProviderKind::from_setting_value(""), None);
    }

    #[test]
    fn api_key_settings_are_distinct_across_all_providers() {
        let mut seen = std::collections::HashSet::new();
        for kind in LlmProviderKind::ALL {
            assert!(
                seen.insert(kind.api_key_setting()),
                "duplicate api_key_setting for {kind:?}"
            );
        }
    }

    #[test]
    fn build_provider_chain_with_no_settings_at_all_has_an_empty_provider_list() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let chain = build_provider_chain(&conn, &reqwest::Client::new()).unwrap();
        assert!(chain.providers_is_empty());
    }

    #[test]
    fn build_provider_chain_puts_the_primary_provider_first() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        settings::set(&conn, PROVIDER_SETTING_KEY, "anthropic").unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "ok_1").unwrap();
        settings::set(&conn, LlmProviderKind::Anthropic.api_key_setting(), "ak_1").unwrap();
        let chain = build_provider_chain(&conn, &reqwest::Client::new()).unwrap();
        assert_eq!(chain.order(), &[LlmProviderKind::Anthropic, LlmProviderKind::OpenAi]);
        assert_eq!(chain.active_model().as_deref(), Some(anthropic::DEFAULT_MODEL));
    }

    #[test]
    fn build_provider_chain_skips_providers_with_no_saved_key() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        settings::set(&conn, PROVIDER_SETTING_KEY, "anthropic").unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "ok_1").unwrap();
        let chain = build_provider_chain(&conn, &reqwest::Client::new()).unwrap();
        assert_eq!(chain.order(), &[LlmProviderKind::OpenAi]);
    }

    #[test]
    fn a_saved_model_from_an_older_generation_is_ignored() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        settings::set(&conn, PROVIDER_SETTING_KEY, "openai").unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "ok_1").unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.model_setting(), "gpt-5.5").unwrap();
        let chain = build_provider_chain(&conn, &reqwest::Client::new()).unwrap();
        assert_eq!(chain.active_model().as_deref(), Some(openai::DEFAULT_MODEL));
        settings::set(&conn, LlmProviderKind::OpenAi.model_setting(), "gpt-6.1-sol").unwrap();
        let chain = build_provider_chain(&conn, &reqwest::Client::new()).unwrap();
        assert_eq!(chain.active_model().as_deref(), Some("gpt-6.1-sol"));
    }

    #[test]
    fn removed_providers_are_unknown_settings() {
        assert_eq!(LlmProviderKind::ALL, [LlmProviderKind::OpenAi, LlmProviderKind::Anthropic]);
        for removed in ["gemini", "groq"] {
            assert_eq!(LlmProviderKind::from_setting_value(removed), None);
        }
    }

    // --- `build_context_string`: the pure retrieval-to-prompt-context logic
    // `retrieve_context` wraps with real `AppHandle`/`Db` I/O. Uses
    // `rag::embed_tfidf::HashedTermFrequencyEmbedder` for real (deterministic,
    // no network) vectors rather than hand-rolled fake ones, so these tests
    // exercise the exact same vector space `retrieve_context` really uses.

    fn tfidf(text: &str) -> Vec<f32> {
        crate::rag::embed_tfidf::vectorize(text)
    }

    #[test]
    fn returns_none_when_there_are_no_candidates_at_all() {
        let query = tfidf("what's your notice period");
        assert_eq!(
            build_context_string(&query, &[], RETRIEVAL_K, RETRIEVAL_THRESHOLD),
            None
        );
    }

    #[test]
    fn returns_the_matching_chunks_text_joined_by_newline() {
        let query = tfidf("what's your notice period");
        let candidates = vec![
            (
                Uuid::from_u128(1),
                "[you] my notice period is two weeks".to_string(),
                tfidf("[you] my notice period is two weeks"),
            ),
            (
                Uuid::from_u128(2),
                "[them] tell me about a challenging project".to_string(),
                tfidf("[them] tell me about a challenging project"),
            ),
        ];
        let context = build_context_string(&query, &candidates, RETRIEVAL_K, RETRIEVAL_THRESHOLD)
            .expect(
                "the notice-period chunk should score above threshold for a notice-period question",
            );
        assert!(context.contains("my notice period is two weeks"));
    }

    #[test]
    fn nothing_above_threshold_is_none_not_an_empty_string() {
        let query = tfidf("what's your notice period");
        // A completely unrelated vocabulary chunk: with a high enough
        // threshold, even the best available match must be excluded.
        let candidates = vec![(
            Uuid::from_u128(1),
            "unrelated text".to_string(),
            tfidf("completely different vocabulary entirely"),
        )];
        assert_eq!(
            build_context_string(&query, &candidates, RETRIEVAL_K, 0.99),
            None
        );
    }

    #[test]
    fn respects_the_k_limit_even_with_many_relevant_candidates() {
        let query = tfidf("notice period question");
        let candidates: Vec<_> = (0..10)
            .map(|i| {
                let text = format!("[you] notice period chunk number {i}");
                (Uuid::from_u128(i), text.clone(), tfidf(&text))
            })
            .collect();
        let context = build_context_string(&query, &candidates, 3, 0.0).unwrap();
        assert_eq!(
            context.lines().count(),
            3,
            "must not exceed k even when every candidate scores above threshold"
        );
    }

    #[test]
    fn answer_started_event_name_is_what_the_panel_listens_for() {
        // Regression: backend used to emit this and the panel never
        // subscribed, so "Generating…" stayed hidden until the first token.
        assert_eq!(EVENT_ANSWER_STARTED, "answer-started");
        assert_eq!(timing::EVENT_ANSWER_TIMING, "answer-timing");
    }

    #[test]
    fn invalidate_makes_the_previous_generation_stale_and_releases_the_lock() {
        let t = AnswerTrigger::default();
        t.in_flight.store(true, Ordering::SeqCst);
        let gen = t.generation.fetch_add(1, Ordering::SeqCst) + 1;
        t.invalidate();
        assert_ne!(t.generation.load(Ordering::SeqCst), gen);
        assert!(!t.in_flight.load(Ordering::SeqCst));
    }
}
