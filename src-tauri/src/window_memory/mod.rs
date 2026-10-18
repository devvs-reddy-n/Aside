//! Live conversation windows for Command+Enter.
//!
//! Everything spoken between two presses is one raw window. The window is
//! frozen at the press; later speech belongs only to the next window.

pub mod background;
mod cache;
mod live;
mod merge;
mod parse;
mod prompt;
mod types;

pub use background::{enqueue_window, init_background_worker};
pub use parse::{ParsedResponse, StreamParser};
#[cfg(test)]
pub(crate) use prompt::build_prompt;
pub use prompt::{AnswerRules, ContextPreview, DefaultContext, LengthOverride, WindowPrompt};
pub use types::{
    ConversationWindow, PressLog, RawTurn, SessionMemory, Speaker, WindowPreview,
    EMPTY_WINDOW_NOTICE,
};

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::audio::channel::Channel;
use crate::storage::Db;

use cache::DefaultsCache;
use live::LiveState;
use types::RawTurn as Turn;

pub const EVENT_WINDOW_MEMORY: &str = "window-memory";

/// `storage::settings` key for the Settings screen's stored replacement for
/// [`STRICT_RULES`]. A plain global setting, not session-scoped — same
/// shape as `stt_engine`/`llm_provider` in `commands/stt.rs`/`commands/llm.rs`,
/// the existing get/set-setting pattern this follows.
const ANSWER_RULES_SETTING_KEY: &str = "answer_rules_override";

#[derive(Default)]
struct Inner {
    open: Vec<Turn>,
    live: LiveState,
    defaults: DefaultsCache,
    open_started_ms: u64,
    memory: SessionMemory,
    press_count: u32,
    last_log: Option<PressLog>,
    last_answer_window: Option<ConversationWindow>,
}

#[derive(Default)]
pub struct WindowMemory(Mutex<Inner>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record one STT event. Finals become committed turns. Interims stay
/// live until freeze (or until a later final replaces them).
pub fn on_transcript(app: &AppHandle, channel: Channel, text: &str, is_final: bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    let at = now_ms();
    if guard.open.is_empty()
        && guard.live.me.interim.is_empty()
        && guard.live.other.interim.is_empty()
    {
        guard.open_started_ms = at;
    }
    let Inner { open, live, .. } = &mut *guard;
    live::apply_event(
        open,
        live,
        Speaker::from_channel(channel),
        trimmed,
        is_final,
        at,
    );
}

pub fn clear(app: &AppHandle) {
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Inner::default();
}

/// A fresh speech window, or the last window when the person presses again.
/// Repeats must not be queued a second time for background memory processing.
pub struct AnswerWindow {
    pub window: ConversationWindow,
    pub is_new: bool,
}

impl Inner {
    fn freeze_for_answer(&mut self, end: u64) -> Option<AnswerWindow> {
        let start = if self.open.is_empty()
            && self.live.me.interim.is_empty()
            && self.live.other.interim.is_empty()
        { end } else { self.open_started_ms };
        let frozen = live::freeze_with_live(&mut self.open, &mut self.live, start, end);
        self.open_started_ms = end;
        if frozen.is_empty() {
            return self.last_answer_window.clone().map(|window| AnswerWindow { window, is_new: false });
        }
        // Remember the input before requesting an answer, so a provider failure
        // can be retried and a length action can reuse the same spoken question.
        self.last_answer_window = Some(frozen.clone());
        Some(AnswerWindow { window: frozen, is_new: true })
    }
}

pub fn freeze_for_answer(app: &AppHandle) -> Option<AnswerWindow> {
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.freeze_for_answer(now_ms())
}

pub fn preview(app: &AppHandle) -> WindowPreview {
    let state = app.state::<WindowMemory>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    let now = now_ms();
    let window = ConversationWindow {
        turns: live::preview_turns(&guard.open, &guard.live, now),
        start_ms: guard.open_started_ms,
        end_ms: now,
    };
    WindowPreview {
        raw_window: window.render(),
        facts: guard.memory.facts.clone(),
        summary: guard.memory.summary.clone(),
        press_count: guard.press_count,
        last_log: guard.last_log.clone(),
    }
}

pub fn invalidate_defaults(app: &AppHandle) {
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.defaults.invalidate();
}

/// Load resume / job / notes into the cache now so Command+Enter does not.
/// Also (re)loads the session's DB-backed `storage::facts` rows — briefing/
/// document/regex-extracted facts, `status.md` 7.9 — into `SessionMemory`
/// so `build_prompt()` actually sees them; called after every action that
/// can add one (`commit_briefing`, document ingest, session create).
pub fn warm_defaults(app: &AppHandle) {
    let active = crate::active_session::get(app);
    let sid = active.as_ref().and_then(|a| a.id);
    let ctx = load_default_context_from_store(app, sid, active);
    let db_facts = sid.map(|sid| load_db_facts(app, sid)).unwrap_or_default();
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.defaults.store(sid, ctx);
    if !db_facts.is_empty() {
        merge::merge_db_facts(&mut guard.memory, db_facts);
    }
}

/// Reads `storage::facts::list_for_session` and renders each row as a
/// `"key: value"` fact string, the same shape any other memory fact takes.
fn load_db_facts(app: &AppHandle, sid: Uuid) -> Vec<String> {
    let db = app.state::<Db>();
    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
    crate::storage::facts::list_for_session(&conn, sid)
        .unwrap_or_default()
        .into_iter()
        .map(|f| format!("{}: {}", f.key, f.value))
        .collect()
}

pub fn build_request(
    app: &AppHandle,
    session_id: Option<Uuid>,
    window: &ConversationWindow,
) -> WindowPrompt {
    build_request_with_length(app, session_id, window, None)
}

/// Same as [`build_request`], with an optional manual [`LengthOverride`]
/// from the overlay's Small/Medium/Long buttons passed through to
/// [`prompt::build_prompt_with_length`].
pub fn build_request_with_length(
    app: &AppHandle,
    session_id: Option<Uuid>,
    window: &ConversationWindow,
    length: Option<LengthOverride>,
) -> WindowPrompt {
    let default_ctx = load_default_context(app, session_id);
    let state = app.state::<WindowMemory>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    prompt::build_prompt_with_length(&default_ctx, &guard.memory, window, length)
}

/// The Documents & Context page's cache readout (`status.md` Layer 1.2):
/// loads the same [`DefaultContext`] a real answer would use for
/// `session_id` (falling back to the active session like every other
/// preview here) and renders it through [`DefaultContext::preview`] — the
/// exact `STRICT_RULES` + `render()` pair a real prompt's `system` string
/// uses, so the number shown is truthful, not re-derived.
pub fn context_preview(app: &AppHandle, session_id: Option<Uuid>) -> ContextPreview {
    load_default_context(app, session_id).preview()
}

/// Settings screen readout for the "Answer rules" editor — the override
/// stored under [`ANSWER_RULES_SETTING_KEY`] when present, [`STRICT_RULES`]
/// otherwise. Reads storage directly rather than the [`DefaultsCache`] a
/// real prompt uses, since this can be called with no session (and
/// therefore no warm cache) at all — a single indexed `settings` lookup is
/// cheap enough that a fresh read here costs nothing meaningful.
pub fn answer_rules(app: &AppHandle) -> AnswerRules {
    let db = app.state::<Db>();
    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
    let stored = stored_rules_override(&conn);
    prompt::describe_rules(stored.as_deref())
}

fn stored_rules_override(conn: &rusqlite::Connection) -> Option<String> {
    crate::storage::settings::get(conn, ANSWER_RULES_SETTING_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
}

/// Validates and stores an Answer Rules override, then re-warms the cached
/// default context so the very next press already reflects it — the same
/// write-then-`warm_defaults` shape `set_session_instructions`/
/// `ingest_document`/`attach_document_to_session` all use, so cache
/// staleness behaves identically everywhere it can happen. Rejects (leaving
/// the previous override, or the built-in rules, untouched) when `text`
/// fails [`prompt::validate_rules_text`] — see that function for exactly
/// what it checks and why.
pub fn set_answer_rules(app: &AppHandle, text: String) -> Result<AnswerRules, String> {
    let trimmed = text.trim();
    prompt::validate_rules_text(trimmed)?;
    {
        let db = app.state::<Db>();
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        crate::storage::settings::set(&conn, ANSWER_RULES_SETTING_KEY, trimmed)?;
    }
    warm_defaults(app);
    Ok(answer_rules(app))
}

/// Deletes any stored override, restoring [`STRICT_RULES`] exactly, then
/// re-warms the cache the same way [`set_answer_rules`] does.
pub fn reset_answer_rules(app: &AppHandle) -> AnswerRules {
    {
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        let _ = crate::storage::settings::delete(&conn, ANSWER_RULES_SETTING_KEY);
    }
    warm_defaults(app);
    answer_rules(app)
}

fn load_default_context(app: &AppHandle, session_id: Option<Uuid>) -> DefaultContext {
    let active = crate::active_session::get(app);
    let sid = session_id.or_else(|| active.as_ref().and_then(|a| a.id));
    {
        let state = app.state::<WindowMemory>();
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = guard.defaults.get(sid) {
            return cached.clone();
        }
    }
    let ctx = load_default_context_from_store(app, sid, active);
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.defaults.store(sid, ctx.clone());
    ctx
}

fn load_default_context_from_store(
    app: &AppHandle,
    sid: Option<Uuid>,
    active: Option<crate::active_session::ActiveSessionInfo>,
) -> DefaultContext {
    let mut ctx = DefaultContext {
        company: active.as_ref().and_then(|a| a.company.clone()),
        job_description: active.as_ref().and_then(|a| a.job_description.clone()),
        instructions: active.as_ref().and_then(|a| a.instructions.clone()),
        ..DefaultContext::default()
    };
    // Read the rules override before the `sid` early-return below — it is a
    // global Settings-screen value (`ANSWER_RULES_SETTING_KEY`), not
    // session-scoped like everything else this function loads, so a
    // sessionless caller (e.g. the Settings screen open with nothing
    // active yet) still gets it.
    let db = app.state::<Db>();
    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
    ctx.rules_override = stored_rules_override(&conn);
    let Some(sid) = sid else {
        return ctx;
    };
    if ctx.instructions.is_none() {
        ctx.instructions = crate::storage::instructions::get(&conn, sid).ok().flatten();
    }
    ctx.target_role = session_target_role(&conn, sid);
    // Prefer the documents the owner actually attached to this session
    // (`commands::documents::attach_document_to_session` / a direct
    // in-session upload) over the oldest-row-wins library scan — this is
    // the fix for a stale/older upload silently outranking the one the
    // owner picked. Empty only for a session that never went through either
    // attach path (e.g. an older session), which is exactly when the old
    // scan should still apply.
    let attached =
        crate::storage::documents::list_attached_for_session(&conn, sid).unwrap_or_default();
    let docs = if attached.is_empty() {
        crate::storage::documents::list_for_context(&conn, sid).unwrap_or_default()
    } else {
        attached
    };
    for doc in docs {
        match doc.doc_type.as_str() {
            "resume" if ctx.resume.is_none() => ctx.resume = Some(doc.raw_text),
            "job_description" if ctx.job_description.is_none() => {
                ctx.job_description = Some(doc.raw_text);
            }
            _ => ctx.other_docs.push((doc.file_name, doc.raw_text)),
        }
    }
    ctx
}

/// The role from the copilot this call was started with (its snapshot is
/// frozen at call start, so later copilot edits don't shift a live call).
fn session_target_role(conn: &rusqlite::Connection, sid: Uuid) -> Option<String> {
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT copilot_snapshot_json FROM sessions WHERE id = ?1",
            [sid.to_string()],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    snapshot
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value.get("target_role")?.as_str().map(str::trim).map(str::to_string))
        .filter(|role| !role.is_empty())
}

/// Records one Command+Enter press's diagnostics after its answer finishes
/// streaming. **Does not touch `memory` at all** — the main model's reply
/// (`parsed`) is plain visible-answer text only now (no `context_update`),
/// and this window's own contribution to memory is instead already queued
/// (`background::enqueue_window`, called from `llm::respond_to_window`
/// right after `freeze_for_answer()`) for the asynchronous cheap-model pipeline in
/// `background.rs` — which is what actually mutates `memory` and fires its
/// own `window-memory` event once it lands. This function only logs and
/// bumps `press_count`.
pub fn note_press(
    app: &AppHandle,
    window: &ConversationWindow,
    parsed: &ParsedResponse,
    encoded_len: usize,
    ttft_ms: Option<u64>,
    total_ms: Option<u64>,
) {
    let state = app.state::<WindowMemory>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    let memory_snapshot = guard.memory.render();
    guard.press_count += 1;
    guard.last_log = Some(PressLog {
        press_number: guard.press_count,
        window_start_ms: window.start_ms,
        window_end_ms: window.end_ms,
        raw_window: window.render(),
        memory_snapshot,
        input_token_estimate: encoded_len.div_ceil(4),
        output_token_estimate: parsed.answer.len().div_ceil(4),
        generated_answer: parsed.answer.clone(),
        ttft_ms,
        total_ms,
    });
    let preview = WindowPreview {
        raw_window: String::new(),
        facts: guard.memory.facts.clone(),
        summary: guard.memory.summary.clone(),
        press_count: guard.press_count,
        last_log: guard.last_log.clone(),
    };
    drop(guard);
    let _ = app.emit(EVENT_WINDOW_MEMORY, preview);
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::window_memory::parse::StreamParser;

    fn speak(state: &mut Inner, text: &str, is_final: bool, at: u64) {
        live::apply_event(&mut state.open, &mut state.live, Speaker::Other, text, is_final, at);
    }

    #[test]
    fn repeat_answer_and_all_lengths_reuse_the_consumed_window() {
        let mut state = Inner::default();
        assert!(state.freeze_for_answer(1).is_none());
        speak(&mut state, "How do you build reliable systems?", true, 2);
        let first = state.freeze_for_answer(3).unwrap();
        assert!(first.is_new);
        let defaults = DefaultContext::default();
        for (index, length) in [None, Some(LengthOverride::Small), Some(LengthOverride::Medium), Some(LengthOverride::Long), None].into_iter().enumerate() {
            let repeated = state.freeze_for_answer(4 + index as u64).unwrap();
            assert_eq!(repeated.window, first.window);
            assert!(!repeated.is_new, "replays must not duplicate background memory");
            let request = prompt::build_prompt_with_length(&defaults, &state.memory, &repeated.window, length);
            assert!(request.user.contains("How do you build reliable systems?"));
            assert_eq!(request.system.contains("ANSWER LENGTH OVERRIDE"), length.is_some());
        }
    }

    #[test]
    fn new_speech_replaces_the_replay_and_a_new_session_forgets_it() {
        let mut state = Inner::default();
        speak(&mut state, "First question?", true, 1);
        state.freeze_for_answer(2).unwrap();
        speak(&mut state, "Second question?", true, 3);
        let next = state.freeze_for_answer(4).unwrap();
        assert!(next.is_new);
        assert_eq!(next.window.render(), "Other: Second question?");
        assert_eq!(state.freeze_for_answer(5).unwrap().window, next.window);
        // This is the same reset used by window_memory::clear at session boundaries.
        state = Inner::default();
        assert!(state.freeze_for_answer(6).is_none());
    }

    #[test]
    fn an_interim_then_its_final_is_not_mistaken_for_a_new_question() {
        let mut state = Inner::default();
        speak(&mut state, "How does it work?", false, 1);
        let first = state.freeze_for_answer(2).unwrap();
        speak(&mut state, "How does it work?", true, 3);
        let repeated = state.freeze_for_answer(4).unwrap();
        assert!(!repeated.is_new);
        assert_eq!(repeated.window, first.window);
    }

    /// Fix C: `storage::facts` rows (briefing/document/regex-extracted, not
    /// touching an `AppHandle` — this codebase has no `tauri::test` mock-app
    /// fixture, same gap `active_session.rs`'s own tests note) must actually
    /// reach `build_prompt()`'s input once loaded into `SessionMemory`, the
    /// same "key: value" shape `load_db_facts` renders them as.
    #[test]
    fn db_backed_facts_reach_the_built_prompt() {
        use crate::domain::Session;
        use crate::storage::{facts, sessions, Db};

        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Test session", 1_000);
        sessions::create(&conn, &session).unwrap();
        facts::upsert_fact(
            &conn,
            session.id,
            "visa_status",
            "STEM OPT",
            None,
            None,
            1_000,
        )
        .unwrap();

        let rows = facts::list_for_session(&conn, session.id).unwrap();
        let db_facts: Vec<String> = rows
            .into_iter()
            .map(|f| format!("{}: {}", f.key, f.value))
            .collect();

        let mut memory = SessionMemory::default();
        merge::merge_db_facts(&mut memory, db_facts);

        let window = ConversationWindow {
            turns: vec![Turn {
                speaker: Speaker::Other,
                text: "What's your visa status?".into(),
                at_ms: 1,
            }],
            start_ms: 1,
            end_ms: 2,
        };
        let prompt = build_prompt(&DefaultContext::default(), &memory, &window);
        assert!(
            prompt.user.contains("STEM OPT"),
            "DB-stored fact must reach the built prompt: {}",
            prompt.user
        );
    }

    #[test]
    fn case_a_unanswered_question_is_the_whole_window() {
        let w = ConversationWindow {
            turns: vec![Turn {
                speaker: Speaker::Other,
                text: "Tell me about your current project.".into(),
                at_ms: 1,
            }],
            start_ms: 1,
            end_ms: 2,
        };
        let p = build_prompt(&DefaultContext::default(), &SessionMemory::default(), &w);
        assert!(p.user.contains("Tell me about your current project."));
        assert!(p.system.contains("entire Current Conversation Window"));
    }

    #[test]
    fn case_c_responsibility_is_still_in_the_same_window_as_answered_tools() {
        let w = ConversationWindow {
            turns: vec![
                Turn {
                    speaker: Speaker::Other,
                    text: "What technology?".into(),
                    at_ms: 1,
                },
                Turn {
                    speaker: Speaker::Me,
                    text: "Kafka.".into(),
                    at_ms: 2,
                },
                Turn {
                    speaker: Speaker::Other,
                    text: "Team size?".into(),
                    at_ms: 3,
                },
                Turn {
                    speaker: Speaker::Me,
                    text: "Eight.".into(),
                    at_ms: 4,
                },
                Turn {
                    speaker: Speaker::Other,
                    text: "Your responsibility?".into(),
                    at_ms: 5,
                },
            ],
            start_ms: 1,
            end_ms: 5,
        };
        let p = build_prompt(&DefaultContext::default(), &SessionMemory::default(), &w);
        assert!(p.user.contains("Kafka."));
        assert!(p.user.contains("Eight."));
        assert!(p.user.contains("Your responsibility?"));
    }

    /// The main model's reply is plain text only now (no `context_update`
    /// JSON, no `---ASIDE_META---` marker) — `StreamParser` just passes
    /// text through, so `note_press` never touches `memory`. See
    /// `parse.rs`'s own tests for the streaming contract itself.
    #[test]
    fn a_plain_text_reply_streams_through_untouched() {
        let mut parser = StreamParser::default();
        parser.push("My main responsibility is owning the Kafka consumers.");
        let parsed = parser.finish();
        assert_eq!(
            parsed.answer,
            "My main responsibility is owning the Kafka consumers."
        );
    }
}
