//! Per-session instructions (tone, "keep answers casual") stored separately
//! from the job description — the backend no longer mashes them into one field.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn upsert(
    conn: &Connection,
    session_id: Uuid,
    text: &str,
    updated_at_ms: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO session_instructions (session_id, text, updated_at_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET
            text = excluded.text,
            updated_at_ms = excluded.updated_at_ms",
        params![session_id.to_string(), text, updated_at_ms],
    )
    .map_err(|e| format!("failed to upsert session instructions: {e}"))?;
    Ok(())
}

pub fn get(conn: &Connection, session_id: Uuid) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT text FROM session_instructions WHERE session_id = ?1",
        params![session_id.to_string()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("failed to load session instructions: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{sessions, Db};

    #[test]
    fn upsert_then_get_round_trips() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        upsert(&conn, session.id, "keep answers casual", 10).unwrap();
        assert_eq!(
            get(&conn, session.id).unwrap().as_deref(),
            Some("keep answers casual")
        );
        upsert(&conn, session.id, "be formal", 20).unwrap();
        assert_eq!(
            get(&conn, session.id).unwrap().as_deref(),
            Some("be formal")
        );
    }
}
