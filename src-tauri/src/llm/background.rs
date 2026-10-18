//! The real (network-calling) half of the cheap, asynchronous "second
//! model" behind the single growing-summary memory design. The
//! queueing/ordering/failure-handling contract lives in
//! `window_memory::background`, which defines the [`window_memory::background::Compressor`]
//! trait this module implements; this file is only the actual HTTP call.
//!
//! **This is the correctness mechanism, not just a cost optimization.**
//! [`LlmCompressor::summarize_window`] is fed only the raw completed
//! transcript window (`Other:`/`Me:` lines) — never the visible answer the
//! main model streamed to the user. `run_job` (in
//! `window_memory::background`) builds that text from `window.render()`
//! alone, so there is no answer text anywhere in scope to pass even by
//! mistake. A compressor that structurally cannot see the AI's own
//! suggested answer cannot leak an unspoken suggestion back into memory as
//! if a real participant had said it — the same "spoken truth beats an AI
//! suggestion" rule the rest of this app is built around, now enforced by
//! this call's own signature rather than by a post-hoc filter (contrast the
//! old `merge::leaks_prior_suggestion`, which this design replaces).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::storage::{settings, Db};
use crate::window_memory::background::Compressor;

use super::{
    build_provider, openai, AnswerSink, HttpClient, LlmProvider, LlmProviderKind,
    SERVICE_TIER_SETTING_KEY,
};

const SUMMARIZE_SYSTEM_PROMPT: &str = "You silently take notes on a live spoken conversation. You will be \
given the raw transcript of one short window of that conversation, with lines labeled \"Other:\" (the \
other participant) and \"Me:\" (the user). Write exactly 1 or 2 plain prose sentences describing only what \
was actually said in this window - no more, no less. Say who said what: what Other asked or shared, and what \
the user said about themselves (write it as \"I said...\"). Keep every technology, tool, company, project \
name, and number exactly as spoken, because later answers must stay consistent with what the user actually \
said. Do not answer any question in it, do not add opinions, do not invent anything that is not present in \
the transcript. Output only the 1-2 sentences, nothing else - no preamble, no labels, no quotation marks.";

/// The one intentional difference between the live rolling note and the
/// history-screen summary: this describes the whole completed transcript in
/// a format the UI can present as a paragraph plus a short list.
const SESSION_SUMMARY_SYSTEM_PROMPT: &str = "You summarize a completed spoken conversation. The transcript uses \
\"Other:\" for the other participant and \"Me:\" for the user. Return only valid JSON in exactly this shape: \
{\"paragraph\":\"...\",\"highlights\":[\"...\"]}. The paragraph must be 2 to 4 factual sentences. \
The highlights must contain 3 to 6 short, factual notes. Use only information actually said in the transcript. \
Do not answer questions, add opinions, speculate, or mention these instructions.";

/// A whole-call summary returned by the history action. This stays separate
/// from the one- or two-sentence rolling background notes persisted during a
/// live call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub paragraph: String,
    pub highlights: Vec<String>,
}

/// The background note-taker always uses OpenAI's `gpt-5.4-mini`
/// ([`openai::BACKGROUND_MODEL`]) with the saved OpenAI key, whichever model
/// answers the call, Claude included. No fallback chain: a failed note is
/// logged and skipped by `window_memory::background::run_job`, never shown.
pub(super) fn build_background_provider(app: &AppHandle) -> Result<Box<dyn LlmProvider>, String> {
    let provider = LlmProviderKind::OpenAi;
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let api_key = settings::get(&conn, provider.api_key_setting())?
        .ok_or_else(|| "background notes need an OpenAI key".to_string())?;
    let model = openai::BACKGROUND_MODEL.to_string();
    let tier = settings::get(&conn, SERVICE_TIER_SETTING_KEY)?;
    drop(conn);
    let client = app.state::<HttpClient>().0.clone();
    Ok(build_provider(provider, client, api_key, Some(model), tier))
}

/// Collects a provider's full streamed reply into one `String` — the
/// background compressor never needs incremental delivery, only the final
/// text once the call completes.
#[derive(Default)]
struct CollectingSink {
    text: String,
    error: Option<String>,
}

impl AnswerSink for CollectingSink {
    fn on_delta(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn on_done(&mut self, _canned: bool) {}

    fn on_error(&mut self, message: &str) {
        self.error = Some(message.to_string());
    }
}

pub(super) async fn run_one_shot(
    provider: &dyn LlmProvider,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let encoded = super::prompt::encode_messages(system, user);
    let mut sink = CollectingSink::default();
    provider
        .stream_answer("background memory compression", Some(&encoded), &mut sink)
        .await?;
    if let Some(err) = sink.error {
        return Err(err);
    }
    Ok(sink.text)
}

fn parse_session_summary(raw: &str) -> Result<SessionSummary, String> {
    let trimmed = raw.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|value| value.trim().strip_suffix("```").unwrap_or(value).trim())
        .unwrap_or(trimmed);
    let summary: SessionSummary = serde_json::from_str(json).map_err(|_| {
        "the AI summary was not in the expected format; please try again".to_string()
    })?;
    if summary.paragraph.trim().is_empty() || summary.highlights.is_empty() {
        return Err("the AI summary was incomplete; please try again".to_string());
    }
    Ok(summary)
}

/// Summarizes an already-finished call on demand, using the same deliberately
/// cheap OpenAI background model and saved key as live context compression.
/// This does not persist or feed its result back into future suggestions.
pub async fn summarize_completed_session(
    app: &AppHandle,
    transcript: &str,
) -> Result<SessionSummary, String> {
    let provider = build_background_provider(app)?;
    let raw = run_one_shot(provider.as_ref(), SESSION_SUMMARY_SYSTEM_PROMPT, transcript).await?;
    parse_session_summary(&raw)
}

/// Real [`Compressor`]: builds a fresh provider from current settings on
/// every call (so a mid-session provider/model/key change is picked up on
/// the very next window), calls it once, and returns the raw text — all
/// rollover/cap/atomicity logic lives in `window_memory::background::run_job`,
/// not here.
pub struct LlmCompressor {
    app: AppHandle,
}

impl LlmCompressor {
    pub fn new(app: &AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

#[async_trait::async_trait]
impl Compressor for LlmCompressor {
    async fn summarize_window(&self, window_text: &str) -> Result<String, String> {
        let provider = build_background_provider(&self.app)?;
        run_one_shot(provider.as_ref(), SUMMARIZE_SYSTEM_PROMPT, window_text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collecting_sink_joins_deltas_and_records_the_last_error() {
        let mut sink = CollectingSink::default();
        sink.on_delta("Hello");
        sink.on_delta(", world.");
        sink.on_error("network blip");
        assert_eq!(sink.text, "Hello, world.");
        assert_eq!(sink.error.as_deref(), Some("network blip"));
    }

    #[test]
    fn background_notes_use_gpt_5_4_mini_which_is_not_an_answer_choice() {
        assert_eq!(openai::BACKGROUND_MODEL, "gpt-5.4-mini");
        assert!(!LlmProviderKind::OpenAi.supports_model(openai::BACKGROUND_MODEL));
    }

    #[test]
    fn session_summary_parser_accepts_the_required_json_shape() {
        let parsed = parse_session_summary(
            "```json\n{\"paragraph\":\"They discussed the role and prior work. The user described a migration.\",\"highlights\":[\"Role discussed\",\"Migration experience\",\"Follow-up planned\"]}\n```",
        )
        .unwrap();
        assert_eq!(parsed.highlights.len(), 3);
        assert!(parsed.paragraph.contains("migration"));
    }
}
