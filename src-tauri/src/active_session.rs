//! In-memory "which session is currently active" state (2026-08-25,
//! the Parakeet-style setup-screen work; the old `panel` window this
//! originally also notified was removed 2026-08-27 — see CURRENT.md).
//! Deliberately **in-memory state, not a `session_id`-keyed database read**:
//! with "Save Transcript" off, a session never gets a database row at all,
//! so a DB lookup couldn't cover that path even if it were otherwise
//! simpler — see `docs/decisions.md`.
//!
//! Lives at the crate root (not inside `commands/`) because it's read from
//! `commands::session`, `commands::stt`, and `llm::mod` (prompt context) —
//! the same reasoning `pipeline::TranscriptionRouting` and
//! `llm::AnswerTrigger` already establish for shared Tauri-managed state
//! that doesn't belong to any one `commands/` submodule.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::domain::SessionType;

/// Everything a live answer/prompt needs about the currently active
/// session. `id: None` means an ephemeral session (no
/// database row) — `session_type`/`company`/`job_description` are still
/// real values in that case, just never persisted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActiveSessionInfo {
    pub id: Option<Uuid>,
    pub session_type: SessionType,
    pub company: Option<String>,
    pub job_description: Option<String>,
    /// L1 session instructions, stored separately from the job post.
    pub instructions: Option<String>,
    /// Whether this session writes its transcript to the local notebook.
    /// Ephemeral sessions always leave this false.
    pub save_transcript: bool,
}

pub const EVENT_SESSION_STARTED: &str = "session-started";
pub const EVENT_SESSION_ENDED: &str = "session-ended";

#[derive(Default)]
pub struct ActiveSession(Mutex<Option<ActiveSessionInfo>>);

/// Stores `info` as the current active session and emits `EVENT_SESSION_STARTED`
/// so a listening window updates live instead of needing a reload.
/// `commands::session::get_active_session` covers the on-mount catch-up read
/// for a window that opens after the session already exists.
pub fn set(app: &AppHandle, info: ActiveSessionInfo) {
    let state = app.state::<ActiveSession>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(info.clone());
    drop(guard);
    // A new session must never inherit the previous one's conversation —
    // the same class of staleness an adversarial review already caught once
    // in `PanelWindow.tsx` (a stale question surviving "End Session" and
    // being re-askable). Cleared on start *and* on end, deliberately: either
    // one alone leaves a window where the old talk is still readable.
    crate::transcript_log::clear(app);
    crate::window_memory::clear(app);
    crate::conversation::clear(app);
    crate::screen::cache::start(app);
    let _ = app.emit(EVENT_SESSION_STARTED, info);
}

/// Pure decision: should ending session `ending_id` clear `current`? Split
/// out from `clear_if_matches` so this is unit-testable without a real
/// `AppHandle` — mirrors `llm::build_context_string`'s established split
/// between pure logic and its `AppHandle`-dependent wrapper. `false` for a
/// mismatched id protects against a stale/late `end_session` call (e.g. a
/// slow request finally landing after the user already started a *new*
/// session) silently clobbering the session that's actually active now.
pub fn should_clear(current: &Option<ActiveSessionInfo>, ending_id: Uuid) -> bool {
    matches!(current, Some(info) if info.id == Some(ending_id))
}

/// Clears the active session only if it's still the one being ended (see
/// [`should_clear`]) — a no-op, including no event emitted, otherwise.
pub fn clear_if_matches(app: &AppHandle, ending_id: Uuid) {
    let state = app.state::<ActiveSession>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    if should_clear(&guard, ending_id) {
        *guard = None;
        drop(guard);
        crate::transcript_log::clear(app);
        crate::window_memory::clear(app);
        crate::conversation::clear(app);
        crate::screen::cache::stop(app);
        let _ = app.emit(EVENT_SESSION_ENDED, ());
    }
}

/// For the ephemeral (no database id) end-session path, where there's
/// nothing to match an id against — clears whatever is active, always.
pub fn clear_unconditionally(app: &AppHandle) {
    let state = app.state::<ActiveSession>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    *guard = None;
    drop(guard);
    crate::transcript_log::clear(app);
    crate::window_memory::clear(app);
    crate::conversation::clear(app);
    crate::screen::cache::stop(app);
    let _ = app.emit(EVENT_SESSION_ENDED, ());
}

/// Reads the current active session, if any — a window's on-mount catch-up
/// read, and `llm::respond_to_question`'s source for company/job_description/
/// session_type context.
pub fn get(app: &AppHandle) -> Option<ActiveSessionInfo> {
    let state = app.state::<ActiveSession>();
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.clone()
}

/// Updates L1 instructions without clearing the live transcript. Must not
/// go through [`set`] — that wipes conversation memory on purpose.
pub fn patch_instructions(app: &AppHandle, session_id: Option<Uuid>, instructions: Option<String>) {
    let state = app.state::<ActiveSession>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(info) = guard.as_mut() {
        if session_id.is_none() || info.id == session_id {
            info.instructions = instructions;
        }
    }
}

/// Updates the active session's job description live, without clearing the
/// live transcript — mirrors `patch_instructions`. Needed by
/// `commands::documents::ingest_text_document`: `window_memory`'s
/// `load_default_context_from_store` prefers the active session's own
/// `job_description` over any document of that type whenever it is already
/// `Some` (set at `create_session` time, e.g. from a saved draft), so a
/// freshly pasted job-description document alone would silently lose to a
/// stale one without this.
pub fn patch_job_description(
    app: &AppHandle,
    session_id: Option<Uuid>,
    job_description: Option<String>,
) {
    let state = app.state::<ActiveSession>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(info) = guard.as_mut() {
        if session_id.is_none() || info.id == session_id {
            info.job_description = job_description;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: Option<Uuid>) -> ActiveSessionInfo {
        ActiveSessionInfo {
            id,
            session_type: SessionType::Interview,
            company: None,
            job_description: None,
            instructions: None,
            save_transcript: true,
        }
    }

    #[test]
    fn should_clear_is_true_when_the_ending_id_matches_the_current_session() {
        let id = Uuid::new_v4();
        assert!(should_clear(&Some(info(Some(id))), id));
    }

    #[test]
    fn should_clear_is_false_for_a_mismatched_id() {
        let current = Uuid::new_v4();
        let ending = Uuid::new_v4();
        assert!(!should_clear(&Some(info(Some(current))), ending));
    }

    #[test]
    fn should_clear_is_false_when_nothing_is_active() {
        assert!(!should_clear(&None, Uuid::new_v4()));
    }

    #[test]
    fn should_clear_is_false_for_an_active_ephemeral_session_with_no_id() {
        // An ephemeral session's `id` is always `None`, which can never
        // equal `Some(ending_id)` — confirms clear_if_matches would never
        // be the right call for the ephemeral path (clear_unconditionally
        // is, and takes no id at all).
        assert!(!should_clear(&Some(info(None)), Uuid::new_v4()));
    }

    // NOTE: `set`/`clear_if_matches`/`clear_unconditionally`/`get` all need
    // a real `tauri::AppHandle` (to reach managed state and to emit), and
    // this codebase has no `tauri::test` mock-app fixture set up anywhere
    // yet (checked before writing this module) — stated honestly rather
    // than invented. `should_clear`'s pure decision above is the part that
    // matters for correctness and is fully covered; the thin `AppHandle`
    // wrappers are exercised by the real app, same category of gap this
    // project already carries for `llm::maybe_trigger`'s own event-emitting
    // half.
}
