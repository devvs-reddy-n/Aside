//! One interviewer question + what the user actually said in reply.
//! The AI suggestion is stored too, but prompts must never substitute it
//! for `actual_user_answer`.
//!
//! `open`/`set_user_answer`/`set_ai_suggestion` (the write side) are
//! `#[cfg(test)]`-only — their one production caller was `conversation`'s
//! now-removed auto-answer path (2026-08-31); `list_recent`/`list_all` (the
//! read side, still used by `window_memory` prompt context and
//! `commands::past_sessions`'s transcript view) are real and kept as-is,
//! but a call started after this change will never actually populate a row
//! for them to read — a pre-existing gap this change didn't create (the
//! write path had already gone unreachable once Copilots started forcing
//! auto-answer off for every session), just makes it permanent instead of
//! toggle-dependent.

#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct InterviewExchange {
    pub id: Uuid,
    pub session_id: Uuid,
    pub interviewer_question: String,
    pub interviewer_turn_id: Option<String>,
    pub ai_suggestion: Option<String>,
    pub actual_user_answer: Option<String>,
    pub opened_at_ms: u64,
    pub answered_at_ms: Option<u64>,
    pub suggestion_at_ms: Option<u64>,
}

fn row_to_exchange(row: &rusqlite::Row) -> rusqlite::Result<InterviewExchange> {
    let id: String = row.get("id")?;
    let session_id: String = row.get("session_id")?;
    Ok(InterviewExchange {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&session_id).unwrap_or_else(|_| Uuid::nil()),
        interviewer_question: row.get("interviewer_question")?,
        interviewer_turn_id: row.get("interviewer_turn_id")?,
        ai_suggestion: row.get("ai_suggestion")?,
        actual_user_answer: row.get("actual_user_answer")?,
        opened_at_ms: row.get("opened_at_ms")?,
        answered_at_ms: row.get("answered_at_ms")?,
        suggestion_at_ms: row.get("suggestion_at_ms")?,
    })
}

const COLS: &str = "id, session_id, interviewer_question, interviewer_turn_id, ai_suggestion, \
     actual_user_answer, opened_at_ms, answered_at_ms, suggestion_at_ms";

#[cfg(test)]
pub fn open(
    conn: &Connection,
    session_id: Uuid,
    interviewer_question: &str,
    interviewer_turn_id: Option<&str>,
    opened_at_ms: u64,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO interview_exchanges
            (id, session_id, interviewer_question, interviewer_turn_id, opened_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            session_id.to_string(),
            interviewer_question,
            interviewer_turn_id,
            opened_at_ms,
        ],
    )
    .map_err(|e| format!("failed to open interview exchange: {e}"))?;
    Ok(id)
}

#[cfg(test)]
pub fn set_user_answer(
    conn: &Connection,
    exchange_id: Uuid,
    actual_user_answer: &str,
    answered_at_ms: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE interview_exchanges
         SET actual_user_answer = ?1, answered_at_ms = ?2
         WHERE id = ?3",
        params![actual_user_answer, answered_at_ms, exchange_id.to_string()],
    )
    .map_err(|e| format!("failed to set actual user answer: {e}"))?;
    Ok(())
}

#[cfg(test)]
pub fn set_ai_suggestion(
    conn: &Connection,
    exchange_id: Uuid,
    suggestion: &str,
    suggestion_at_ms: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE interview_exchanges
         SET ai_suggestion = ?1, suggestion_at_ms = ?2
         WHERE id = ?3",
        params![suggestion, suggestion_at_ms, exchange_id.to_string()],
    )
    .map_err(|e| format!("failed to set ai suggestion: {e}"))?;
    Ok(())
}

#[cfg(test)]
pub fn latest(conn: &Connection, session_id: Uuid) -> Result<Option<InterviewExchange>, String> {
    conn.query_row(
        &format!(
            "SELECT {COLS} FROM interview_exchanges
             WHERE session_id = ?1
             ORDER BY opened_at_ms DESC LIMIT 1"
        ),
        params![session_id.to_string()],
        row_to_exchange,
    )
    .optional()
    .map_err(|e| format!("failed to load latest exchange: {e}"))
}

/// Oldest-first, capped. Default prompt depth is the last 1–3.
pub fn list_recent(
    conn: &Connection,
    session_id: Uuid,
    limit: usize,
) -> Result<Vec<InterviewExchange>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM interview_exchanges
             WHERE session_id = ?1
             ORDER BY opened_at_ms DESC LIMIT ?2"
        ))
        .map_err(|e| format!("failed to prepare exchange list: {e}"))?;
    let rows = stmt
        .query_map(
            params![session_id.to_string(), limit as i64],
            row_to_exchange,
        )
        .map_err(|e| format!("failed to run exchange list: {e}"))?;
    let mut out = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read exchange row: {e}"))?;
    out.reverse();
    Ok(out)
}

/// Every exchange for `session_id`, oldest first, no cap — for a past
/// session's transcript view (2026-08-30, owner request: interleave the
/// AI's Question/Answer with the spoken transcript), which needs the whole
/// history, not `list_recent`'s deliberately-capped "last few for the
/// prompt" window.
pub fn list_all(conn: &Connection, session_id: Uuid) -> Result<Vec<InterviewExchange>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM interview_exchanges
             WHERE session_id = ?1
             ORDER BY opened_at_ms ASC"
        ))
        .map_err(|e| format!("failed to prepare full exchange list: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_exchange)
        .map_err(|e| format!("failed to run full exchange list: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read exchange row: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{sessions, Db};

    #[test]
    fn open_then_user_answer_does_not_copy_the_ai_suggestion() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        let id = open(&conn, session.id, "what is your visa status?", None, 10).unwrap();
        set_ai_suggestion(&conn, id, "I am on H-1B", 11).unwrap();
        set_user_answer(&conn, id, "I'm on STEM OPT", 12).unwrap();
        let loaded = latest(&conn, session.id).unwrap().unwrap();
        assert_eq!(
            loaded.actual_user_answer.as_deref(),
            Some("I'm on STEM OPT")
        );
        assert_eq!(loaded.ai_suggestion.as_deref(), Some("I am on H-1B"));
        assert_ne!(loaded.actual_user_answer, loaded.ai_suggestion);
    }

    #[test]
    fn list_recent_caps_and_returns_oldest_of_the_window_first() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        for i in 1..=5 {
            open(&conn, session.id, &format!("Q{i}"), None, i * 10).unwrap();
        }
        let recent = list_recent(&conn, session.id, 3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].interviewer_question, "Q3");
        assert_eq!(recent[2].interviewer_question, "Q5");
        assert!(!recent.iter().any(|e| e.interviewer_question == "Q1"));
    }

    #[test]
    fn list_all_returns_every_exchange_oldest_first_with_no_cap() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        for i in 1..=5 {
            open(&conn, session.id, &format!("Q{i}"), None, i * 10).unwrap();
        }
        let all = list_all(&conn, session.id).unwrap();
        assert_eq!(
            all.iter()
                .map(|e| e.interviewer_question.as_str())
                .collect::<Vec<_>>(),
            vec!["Q1", "Q2", "Q3", "Q4", "Q5"]
        );
    }
}
