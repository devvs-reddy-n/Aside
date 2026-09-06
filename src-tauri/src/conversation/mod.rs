//! Conversation coordinator: tracks both ears' live text and per-turn
//! timing marks.
//!
//! Mic speech is remembered as a live buffer but never triggers anything by
//! itself. The only way Aside ever answers is an explicit trigger — Command+
//! Enter (`llm::respond_to_window`, freezes the live conversation window) or
//! Layer 5 Mic submit (answers the last thing you said) — never automatic
//! send-on-detected-question. An earlier "auto-answer" mode used to make
//! `decide()` dispatch the LLM itself when an interviewer utterance finished;
//! it was removed (2026-08-31, owner request — the feature had already
//! become unreachable: every real session is created through Copilots, which
//! always started calls with it forced off) along with the question/restart/
//! continue detection that only existed to serve it. What's left here is
//! genuinely still used by the manual answer paths: the live per-channel
//! text buffers and the turn-timing marks that feed every answer's KPI
//! summary (`note_answer_done`).

pub mod ack;
pub mod channel_text;
pub mod hypothesis;
pub mod kpi;
pub mod similarity;

use std::sync::Mutex;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::audio::channel::Channel;
use crate::storage::Db;

use ack::is_ack_only;
use channel_text::ChannelBuffer;
use hypothesis::is_sufficiently_understood;
use kpi::TurnMarks;

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub them: ChannelBuffer,
    pub you: ChannelBuffer,
    pub previous_answer_summary: Option<String>,
}

#[derive(Default)]
pub struct Conversation {
    pub memory: Mutex<SessionState>,
    pub marks: Mutex<TurnMarks>,
}

fn buffer_mut(state: &mut SessionState, channel: Channel) -> &mut ChannelBuffer {
    match channel {
        Channel::Mic => &mut state.you,
        Channel::System => &mut state.them,
    }
}

/// Updates the live per-channel buffer and turn-timing marks for one
/// transcript event. No longer decides anything beyond that — see the
/// module doc comment for why (this used to also decide whether to
/// automatically dispatch the LLM).
pub fn decide(
    state: &mut SessionState,
    marks: &mut TurnMarks,
    channel: Channel,
    text: &str,
    is_final: bool,
    now: Instant,
) {
    // A whole-turn "okay" must not merge into the previous question.
    if is_ack_only(text) {
        return;
    }

    let buf = buffer_mut(state, channel);
    buf.push(text, is_final);
    let live = buf.live();
    if live.is_empty() {
        return;
    }

    if marks.turn_started_at.is_none() || (marks.question_end_at.is_some() && !is_final) {
        marks.begin_turn(channel, now);
    }
    marks.mark_first_partial(now);

    if is_sufficiently_understood(channel, &live, is_final) {
        marks.mark_identifiable(now);
    }
    if is_final {
        marks.mark_question_end(now);
    }
}

pub fn clear(app: &AppHandle) {
    let state = app.state::<Conversation>();
    *state.memory.lock().unwrap_or_else(|p| p.into_inner()) = SessionState::default();
    *state.marks.lock().unwrap_or_else(|p| p.into_inner()) = TurnMarks::default();
    crate::llm::invalidate_answers(app);
}

pub fn on_transcript(
    app: &AppHandle,
    channel: Channel,
    _session_id: Option<Uuid>,
    text: &str,
    is_final: bool,
) {
    let now = Instant::now();
    let state = app.state::<Conversation>();
    let mut memory = state.memory.lock().unwrap_or_else(|p| p.into_inner());
    let mut marks = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    decide(&mut memory, &mut marks, channel, text, is_final, now);
}

/// Records a suggested answer against the session's running topic summary.
/// Still used by every real (manual) answer — `llm::answer_with_claim` calls
/// this after every successful stream.
pub fn note_ai_suggestion(
    app: &AppHandle,
    session_id: Option<Uuid>,
    question: &str,
    suggestion: &str,
) {
    let Some(sid) = session_id else {
        return;
    };
    let db = app.state::<Db>();
    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
    let now = now_ms();
    let _ = crate::storage::suggestions::insert(&conn, sid, None, question, suggestion, now);
    refresh_topic(&conn, sid, now);
}

fn refresh_topic(conn: &rusqlite::Connection, session_id: Uuid, now: u64) {
    let Ok(rows) = crate::storage::exchanges::list_recent(conn, session_id, 3) else {
        return;
    };
    let pairs: Vec<(String, Option<String>)> = rows
        .into_iter()
        .map(|e| (e.interviewer_question, e.actual_user_answer))
        .collect();
    let text =
        crate::storage::topics::extractive_from_exchanges(&pairs, crate::storage::topics::CHAR_CAP);
    let _ = crate::storage::topics::upsert(conn, session_id, &text, now);
}

pub fn note_llm_dispatched(app: &AppHandle) {
    let state = app.state::<Conversation>();
    let mut marks = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    marks.mark_llm_dispatched(Instant::now());
}

#[allow(dead_code)]
pub fn note_first_token(app: &AppHandle) {
    let state = app.state::<Conversation>();
    let mut marks = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    if marks.first_token_at.is_none() {
        marks.first_token_at = Some(Instant::now());
    }
}

#[allow(dead_code)]
pub fn note_first_useful(app: &AppHandle) {
    let state = app.state::<Conversation>();
    let mut marks = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    if marks.first_useful_at.is_none() {
        marks.first_useful_at = Some(Instant::now());
    }
}

pub fn note_answer_done(app: &AppHandle, answer_summary: Option<String>) {
    let state = app.state::<Conversation>();
    if let Some(summary) = answer_summary {
        let mut memory = state.memory.lock().unwrap_or_else(|p| p.into_inner());
        memory.previous_answer_summary = Some(summary);
    }
    let mut marks = state.marks.lock().unwrap_or_else(|p| p.into_inner());
    marks.complete_at = Some(Instant::now());
    let summary = kpi::summarize(&marks);
    eprintln!("{}", kpi::log_line(&summary));
    let _ = app.emit(kpi::EVENT_PIPELINE_KPI, summary);
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_partial(state: &mut SessionState, marks: &mut TurnMarks, text: &str) {
        decide(state, marks, Channel::System, text, false, Instant::now());
    }

    fn system_final(state: &mut SessionState, marks: &mut TurnMarks, text: &str) {
        decide(state, marks, Channel::System, text, true, Instant::now());
    }

    #[test]
    fn a_partial_marks_first_partial_but_not_question_end() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        system_partial(&mut state, &mut marks, "what is your notice period");
        assert!(marks.first_partial_at.is_some());
        assert!(marks.question_end_at.is_none());
        assert!(marks.llm_dispatched_at.is_none());
    }

    #[test]
    fn a_final_question_marks_question_end() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        system_final(&mut state, &mut marks, "what is your notice period?");
        assert!(marks.question_end_at.is_some());
        assert!(marks.question_identifiable_at.is_some());
        // Nothing dispatches on its own any more — only an explicit trigger
        // (Command+Enter / Mic submit) ever calls `note_llm_dispatched`.
        assert!(marks.llm_dispatched_at.is_none());
    }

    #[test]
    fn mic_text_buffers_into_you_not_them() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        decide(
            &mut state,
            &mut marks,
            Channel::Mic,
            "what I did at my last job was lead the rewrite",
            true,
            Instant::now(),
        );
        assert!(state.you.live().contains("last job"));
        assert!(state.them.live().is_empty());
    }

    #[test]
    fn system_text_buffers_into_them_not_you() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        system_partial(&mut state, &mut marks, "what is your notice period");
        assert!(state.them.live().contains("notice"));
        assert!(state.you.live().is_empty());
    }

    #[test]
    fn ack_only_text_does_not_mark_a_turn() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        system_final(&mut state, &mut marks, "okay");
        assert!(marks.question_end_at.is_none());
        assert!(state.them.live().is_empty());
    }

    #[test]
    fn growing_partials_keep_updating_the_live_buffer() {
        let mut state = SessionState::default();
        let mut marks = TurnMarks::default();
        system_partial(&mut state, &mut marks, "what is your experience with rust");
        system_partial(
            &mut state,
            &mut marks,
            "what is your experience with rust and distributed systems",
        );
        assert!(state.them.live().contains("distributed systems"));
        assert!(marks.llm_dispatched_at.is_none());
    }
}
