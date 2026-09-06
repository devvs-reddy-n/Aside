//! In-memory rolling record of everything said this session (2026-08-25).
//!
//! The owner asked for "every piece of talk" to reach the AI alongside the
//! current question. Before this, the conversation was **never** sent —
//! `llm::respond_to_question` built context from a one-sentence session
//! block plus at most four RAG-retrieved chunks, and
//! `storage::transcripts::list_for_session` existed but nothing in `llm/`
//! ever called it.
//!
//! **Why in-memory rather than reading the database back.** Two reasons,
//! both real:
//!
//! 1. An ephemeral session ("Save Transcript" off) never writes a row at
//!    all, so a database-backed design would need this buffer anyway for
//!    that path — and then there would be two sources that can drift.
//! 2. `list_for_session` takes the global `Db` mutex and scans the whole
//!    session, on the exact hot path the same session is trying to make
//!    faster.
//!
//! Honest trade-off, recorded in `docs/decisions.md`: this is lost if the
//! app restarts mid-session. Aside has never resumed a live session across a
//! restart, and the durable `chunks` table still backs RAG recall, so
//! nothing that survived before stops surviving now.
//!
//! Lives at the crate root, not inside `commands/`, for the same reason
//! `active_session.rs` does: it's shared Tauri-managed state read from
//! `pipeline`, `llm`, and `shortcuts` alike.

use std::collections::VecDeque;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::audio::channel::Channel;

/// Roughly an hour of brisk two-way conversation. A cap exists at all so a
/// very long session can't grow memory without bound; it is deliberately
/// far above the per-answer context budget (`llm::transcript_context`),
/// which does its own, much tighter trimming at prompt-build time.
const CAPACITY: usize = 600;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Turn {
    pub channel: Channel,
    pub text: String,
}

#[derive(Default)]
pub struct TranscriptLog(Mutex<VecDeque<Turn>>);

/// Pure: the eviction rule, split out so it's testable without an
/// `AppHandle` — the same split `active_session::should_clear` established.
pub fn push_bounded(dq: &mut VecDeque<Turn>, turn: Turn, cap: usize) {
    if cap == 0 {
        return;
    }
    if dq.len() >= cap {
        dq.pop_front();
    }
    dq.push_back(turn);
}

/// Records one final transcript. Called from `pipeline::emit_all` for
/// **both** channels and regardless of whether the session has a database
/// row — an ephemeral session's talk is exactly as useful to the AI as a
/// saved one's.
pub fn append(app: &AppHandle, channel: Channel, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let state = app.state::<TranscriptLog>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    push_bounded(
        &mut guard,
        Turn {
            channel,
            text: trimmed.to_string(),
        },
        CAPACITY,
    );
}

/// Everything said so far, oldest first.
#[allow(dead_code)]
pub fn snapshot(app: &AppHandle) -> Vec<Turn> {
    let state = app.state::<TranscriptLog>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.iter().cloned().collect()
}

/// Newest turn on `channel`, scanning newest-first. Split out so the
/// Cmd+Enter / Answer-button sources are unit-testable without an AppHandle.
pub fn last_turn_on(turns: &VecDeque<Turn>, channel: Channel) -> Option<String> {
    turns
        .iter()
        .rev()
        .find(|t| t.channel == channel)
        .map(|t| t.text.clone())
}

/// Newest `n` turns on `channel`, newest first.
pub fn recent_turns_on(turns: &VecDeque<Turn>, channel: Channel, n: usize) -> Vec<String> {
    turns
        .iter()
        .rev()
        .filter(|t| t.channel == channel)
        .take(n)
        .map(|t| t.text.clone())
        .collect()
}

/// The most recent thing the *other* person said — what the Answer *button*
/// should answer. Cmd+Enter uses [`last_you_turn`] instead (Mode B).
pub fn last_them_turn(app: &AppHandle) -> Option<String> {
    let state = app.state::<TranscriptLog>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    last_turn_on(&guard, Channel::System)
}

pub fn recent_them_turns(app: &AppHandle, n: usize) -> Vec<String> {
    let state = app.state::<TranscriptLog>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    recent_turns_on(&guard, Channel::System, n)
}

/// The most recent thing *you* said — what global Cmd+Enter answers.
pub fn last_you_turn(app: &AppHandle) -> Option<String> {
    let state = app.state::<TranscriptLog>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    last_turn_on(&guard, Channel::Mic)
}

pub fn pop_last_on(turns: &mut VecDeque<Turn>, channel: Channel) -> Option<Turn> {
    let idx = turns.iter().rposition(|t| t.channel == channel)?;
    turns.remove(idx)
}

pub fn pop_last_you(app: &AppHandle) -> Option<Turn> {
    let state = app.state::<TranscriptLog>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    pop_last_on(&mut guard, Channel::Mic)
}

/// Wipes the log. Called when a session starts *and* when one ends, so a new
/// session can never inherit the previous one's conversation — the same
/// class of staleness bug an adversarial review already caught once in
/// `PanelWindow.tsx` (a stale question surviving "End Session").
pub fn clear(app: &AppHandle) {
    let state = app.state::<TranscriptLog>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(channel: Channel, text: &str) -> Turn {
        Turn {
            channel,
            text: text.to_string(),
        }
    }

    #[test]
    fn push_bounded_appends_in_order_below_the_cap() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::System, "first"), 10);
        push_bounded(&mut dq, turn(Channel::Mic, "second"), 10);
        assert_eq!(dq.len(), 2);
        assert_eq!(dq[0].text, "first");
        assert_eq!(dq[1].text, "second");
    }

    #[test]
    fn push_bounded_evicts_the_oldest_at_capacity() {
        let mut dq = VecDeque::new();
        for i in 0..5 {
            push_bounded(&mut dq, turn(Channel::System, &format!("turn {i}")), 3);
        }
        assert_eq!(dq.len(), 3, "must never exceed the cap");
        assert_eq!(dq[0].text, "turn 2", "the oldest must be the one dropped");
        assert_eq!(dq[2].text, "turn 4");
    }

    #[test]
    fn push_bounded_with_a_zero_cap_stores_nothing_rather_than_panicking() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::Mic, "x"), 0);
        assert!(dq.is_empty());
    }

    #[test]
    fn the_real_capacity_constant_is_large_enough_to_be_useful() {
        // Guards against someone "tidying" this down to a tiny number and
        // silently truncating a normal-length interview.
        assert!(CAPACITY >= 200, "a real interview is hundreds of turns");
    }

    // NOTE: `append`/`snapshot`/`last_them_turn`/`clear` all need a real
    // `tauri::AppHandle` to reach managed state, and this codebase has no
    // `tauri::test` mock-app fixture (checked). Their logic is a thin wrapper
    // over `push_bounded` (covered above) plus a `VecDeque` scan; the
    // scanning half is covered by `last_them_turn_logic` below against the
    // same data structure. Same honest gap `active_session.rs` already
    // documents for its own `AppHandle` wrappers.

    #[test]
    fn last_them_turn_logic_finds_the_newest_system_turn_ignoring_the_users_own_speech() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::System, "tell me about yourself"), 10);
        push_bounded(
            &mut dq,
            turn(Channel::Mic, "sure, I lead the audio work"),
            10,
        );
        push_bounded(
            &mut dq,
            turn(Channel::System, "what was the hardest part"),
            10,
        );
        push_bounded(&mut dq, turn(Channel::Mic, "the device switching"), 10);

        let found = last_turn_on(&dq, Channel::System);
        assert_eq!(found, Some("what was the hardest part".to_string()));
    }

    #[test]
    fn last_them_turn_logic_is_none_when_only_the_user_has_spoken() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::Mic, "testing one two"), 10);
        let found = last_turn_on(&dq, Channel::System);
        assert_eq!(found, None, "the user's own speech is never 'the question'");
    }

    #[test]
    fn last_you_turn_logic_finds_the_newest_mic_turn_ignoring_them() {
        let mut dq = VecDeque::new();
        push_bounded(
            &mut dq,
            turn(Channel::System, "what is your notice period"),
            10,
        );
        push_bounded(
            &mut dq,
            turn(Channel::Mic, "can you remind me of my visa"),
            10,
        );
        push_bounded(&mut dq, turn(Channel::System, "and your start date"), 10);
        assert_eq!(
            last_turn_on(&dq, Channel::Mic),
            Some("can you remind me of my visa".to_string())
        );
    }

    #[test]
    fn last_you_turn_logic_is_none_when_only_they_have_spoken() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::System, "tell me about yourself"), 10);
        assert_eq!(last_turn_on(&dq, Channel::Mic), None);
    }

    #[test]
    fn recent_them_turns_returns_newest_first_up_to_n() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::System, "one"), 10);
        push_bounded(&mut dq, turn(Channel::Mic, "me"), 10);
        push_bounded(&mut dq, turn(Channel::System, "two"), 10);
        push_bounded(&mut dq, turn(Channel::System, "three"), 10);
        assert_eq!(
            recent_turns_on(&dq, Channel::System, 2),
            vec!["three".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn pop_last_you_removes_only_the_newest_mic_turn() {
        let mut dq = VecDeque::new();
        push_bounded(&mut dq, turn(Channel::Mic, "first me"), 10);
        push_bounded(&mut dq, turn(Channel::System, "them"), 10);
        push_bounded(&mut dq, turn(Channel::Mic, "echo"), 10);
        let popped = pop_last_on(&mut dq, Channel::Mic).unwrap();
        assert_eq!(popped.text, "echo");
        assert_eq!(
            last_turn_on(&dq, Channel::Mic),
            Some("first me".to_string())
        );
        assert_eq!(last_turn_on(&dq, Channel::System), Some("them".to_string()));
    }
}
