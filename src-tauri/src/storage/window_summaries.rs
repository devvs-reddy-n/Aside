//! Persisted per-session window summaries (2026-08-30, owner request) —
//! `window_memory::SessionMemory.summary` itself lives only in a single
//! process-wide `Mutex`, wiped on every session change (`active_session::
//! clear`), so a past session's growing "conversation so far" list could
//! never be looked at again once that session ended. This table is the
//! durable copy: one row per completed conversation window, in the exact
//! order they were appended, so a session's transcript view can show
//! "what Aside's background summarizer took away from this call" even long
//! after the call itself.
//!
//! Insert-only, like `storage/transcripts.rs` — a window summary is never
//! edited or replaced once written.

use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowSummaryRow {
    pub seq: i64,
    pub text: String,
    pub created_at_ms: u64,
}

/// Appends one summary line for `session_id` at the next sequence number.
/// `seq` is computed here (max existing + 1, or 0) rather than passed in,
/// so a caller can never accidentally create a gap or duplicate — the same
/// reasoning `storage::transcripts`'s insert helpers use.
pub fn append(conn: &Connection, session_id: Uuid, text: &str, now_ms: u64) -> Result<(), String> {
    let next_seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM session_window_summaries WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|e| format!("failed to compute next window-summary sequence: {e}"))?;
    conn.execute(
        "INSERT INTO session_window_summaries (id, session_id, seq, text, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::new_v4().to_string(),
            session_id.to_string(),
            next_seq,
            text,
            now_ms as i64,
        ],
    )
    .map_err(|e| format!("failed to save window summary: {e}"))?;
    Ok(())
}

/// Every summary line for `session_id`, oldest first — the exact order
/// they were appended in, matching how the live "Conversation so far" list
/// already reads.
pub fn list_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<WindowSummaryRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT seq, text, created_at_ms FROM session_window_summaries
             WHERE session_id = ?1 ORDER BY seq ASC",
        )
        .map_err(|e| format!("failed to prepare window-summary query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], |row| {
            Ok(WindowSummaryRow {
                seq: row.get(0)?,
                text: row.get(1)?,
                created_at_ms: row.get::<_, i64>(2)? as u64,
            })
        })
        .map_err(|e| format!("failed to run window-summary query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read a window-summary row: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    fn insert_session(conn: &Connection, id: Uuid) {
        conn.execute(
            "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms)
             VALUES (?1, 't', '[]', 0, 'active', 0, 0)",
            params![id.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn appended_summaries_come_back_in_the_order_they_were_written() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let id = Uuid::new_v4();
        insert_session(&conn, id);
        append(&conn, id, "first window", 100).unwrap();
        append(&conn, id, "second window", 200).unwrap();
        append(&conn, id, "third window", 300).unwrap();
        let rows = list_for_session(&conn, id).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            vec!["first window", "second window", "third window"]
        );
        assert_eq!(
            rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn a_session_with_no_summaries_yet_returns_an_empty_list_not_an_error() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let id = Uuid::new_v4();
        insert_session(&conn, id);
        assert_eq!(list_for_session(&conn, id).unwrap(), vec![]);
    }

    #[test]
    fn summaries_do_not_leak_between_sessions() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        insert_session(&conn, a);
        insert_session(&conn, b);
        append(&conn, a, "belongs to a", 0).unwrap();
        append(&conn, b, "belongs to b", 0).unwrap();
        assert_eq!(list_for_session(&conn, a).unwrap().len(), 1);
        assert_eq!(list_for_session(&conn, b).unwrap().len(), 1);
    }
}
