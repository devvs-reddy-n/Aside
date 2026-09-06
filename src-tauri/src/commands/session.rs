use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::active_session::{self, ActiveSessionInfo};
use crate::domain::{validate_new_session, Session, SessionType};
use crate::storage::{sessions, Db};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn pick_nonempty(a: Option<String>, b: Option<String>) -> Option<String> {
    [a, b]
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

/// `company`/`job_description`/`session_type`/`save_transcript`
/// are all optional so `SttBench.tsx`'s existing title-only call keeps
/// compiling and behaving exactly as before (every new field falls back to
/// `Session::new`'s own defaults, which already preserve prior behavior —
/// see that function's doc comments). `instructions` is L1 tone/rules;
/// `session_instructions` is the same field under the older frontend name.
/// `app` is new here (needed to publish the created session as the active
/// one, `active_session::set`, so the `panel` window picks it up) — Tauri
/// injects it automatically, no frontend change needed for this parameter,
/// same as `end_session` below.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_session(
    app: AppHandle,
    db: State<Db>,
    title: String,
    company: Option<String>,
    job_description: Option<String>,
    instructions: Option<String>,
    session_instructions: Option<String>,
    session_type: Option<SessionType>,
    save_transcript: Option<bool>,
) -> Result<Session, String> {
    let mut session = Session::new(title, now_ms());
    session.company = company;
    session.job_description = job_description;
    if let Some(kind) = session_type {
        session.session_type = kind;
    }
    if let Some(save) = save_transcript {
        session.save_transcript = save;
    }
    let ins = pick_nonempty(instructions, session_instructions);
    validate_new_session(&session).map_err(|e| e.to_string())?;
    let conn =
        db.0.lock()
            .map_err(|_| "database state poisoned".to_string())?;
    sessions::create(&conn, &session)?;
    if let Some(text) = ins.as_deref() {
        crate::storage::instructions::upsert(&conn, session.id, text, now_ms())?;
    }
    drop(conn);
    active_session::set(
        &app,
        ActiveSessionInfo {
            id: Some(session.id),
            session_type: session.session_type,
            company: session.company.clone(),
            job_description: session.job_description.clone(),
            instructions: ins,
            save_transcript: session.save_transcript,
        },
    );
    crate::window_memory::warm_defaults(&app);
    Ok(session)
}

/// The panel's on-mount catch-up read — covers the case where the setup
/// screen created a session before the panel window ever mounted, which
/// the `session-started` event alone (fired once, at creation time) can't
/// reach.
#[tauri::command]
pub fn get_active_session(app: AppHandle) -> Option<ActiveSessionInfo> {
    active_session::get(&app)
}

/// Thin write for the Create screen's Instructions modal. Does not touch
/// the job description.
#[tauri::command]
pub fn set_session_instructions(
    app: AppHandle,
    db: State<Db>,
    session_id: Uuid,
    instructions: String,
) -> Result<(), String> {
    let trimmed = instructions.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let conn =
        db.0.lock()
            .map_err(|_| "database state poisoned".to_string())?;
    crate::storage::instructions::upsert(&conn, session_id, trimmed, now_ms())?;
    drop(conn);
    active_session::patch_instructions(&app, Some(session_id), Some(trimmed.to_string()));
    crate::window_memory::warm_defaults(&app);
    Ok(())
}

#[tauri::command]
pub fn get_window_preview(app: AppHandle) -> crate::window_memory::WindowPreview {
    crate::window_memory::preview(&app)
}

/// The Documents & Context page's cache readout (Layer 1.2). `session_id`
/// is optional and falls back to the active session, same as every other
/// preview command here — lets the page show a live number before a
/// session even exists yet.
#[tauri::command]
pub fn get_context_preview(
    app: AppHandle,
    session_id: Option<Uuid>,
) -> crate::window_memory::ContextPreview {
    crate::window_memory::context_preview(&app, session_id)
}

/// Settings screen's "Answer rules" editor — current readout (override if
/// stored, the built-in rules otherwise).
#[tauri::command]
pub fn get_answer_rules(app: AppHandle) -> crate::window_memory::AnswerRules {
    crate::window_memory::answer_rules(&app)
}

/// Saves a new Answer Rules override behind the Settings screen's warning.
/// Rejected only when `text` is empty/whitespace-only — see
/// `window_memory::prompt::validate_rules_text` for exactly what that
/// checks. Conversation memory no longer depends on anything in this text
/// (2026-08-29 rewrite — it's produced by a separate, asynchronous
/// cheap-model pipeline, `window_memory::background`, that never reads
/// these rules at all), so there is no marker/JSON shape left to require
/// here. Not length-gated either: a short save still succeeds, just with
/// `meets_cache_floor: false` on the returned readout, so the screen can
/// warn without blocking the owner's own call.
#[tauri::command]
pub fn set_answer_rules(
    app: AppHandle,
    text: String,
) -> Result<crate::window_memory::AnswerRules, String> {
    crate::window_memory::set_answer_rules(&app, text)
}

/// Deletes the stored override, restoring the built-in rules exactly.
#[tauri::command]
pub fn reset_answer_rules(app: AppHandle) -> crate::window_memory::AnswerRules {
    crate::window_memory::reset_answer_rules(&app)
}

/// `length` is the overlay's optional per-press Small/Medium/Long override
/// (2026-08-31) — `None` for the plain Answer button/⌘↩, which stays AUTO.
#[tauri::command]
pub async fn trigger_window_answer(
    app: AppHandle,
    length: Option<crate::window_memory::LengthOverride>,
) -> Result<(), String> {
    let session_id = crate::active_session::get(&app).and_then(|s| s.id);
    crate::llm::respond_to_window(app, session_id, length).await
}

/// The overlay's Chat view needs the real, two-sided conversation, not just
/// answer history. These turns are kept in memory for the active call and
/// are cleared when it ends, matching the rest of the live helper.
#[tauri::command]
pub fn get_live_transcript(app: AppHandle) -> Vec<crate::transcript_log::Turn> {
    crate::transcript_log::snapshot(&app)
}

/// `app` is Phase 7's incremental-indexing plumbing: ending a session
/// flushes whatever partial chunk each channel's `StreamingChunker` was
/// still holding (`rag::indexer::flush_session`) so the last few words
/// spoken don't sit un-indexed forever. Tauri injects `AppHandle`
/// automatically — no frontend change needed for this parameter.
#[tauri::command]
pub fn end_session(app: AppHandle, db: State<Db>, id: Uuid) -> Result<(), String> {
    let conn =
        db.0.lock()
            .map_err(|_| "database state poisoned".to_string())?;
    sessions::end(&conn, id, now_ms())?;
    drop(conn);
    crate::rag::indexer::flush_session(&app, id);
    active_session::clear_if_matches(&app, id);
    crate::window_memory::invalidate_defaults(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::Session;
    use crate::storage::{sessions, Db};

    #[test]
    fn create_then_get_defaults_save_transcript_to_true() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("plain session", 1_000);
        sessions::create(&conn, &session).unwrap();
        let loaded = sessions::get(&conn, session.id).unwrap().unwrap();
        assert!(loaded.save_transcript, "save_transcript must default true");
    }

    #[test]
    fn create_then_get_round_trips_save_transcript_off() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut session = Session::new("no-save round", 1_000);
        session.save_transcript = false;
        sessions::create(&conn, &session).unwrap();
        let loaded = sessions::get(&conn, session.id).unwrap().unwrap();
        assert!(!loaded.save_transcript);
    }
}
