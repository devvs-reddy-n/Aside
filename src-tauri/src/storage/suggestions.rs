//! Stored AI suggestions, keyed to an interview exchange when one exists.

use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn insert(
    conn: &Connection,
    session_id: Uuid,
    exchange_id: Option<Uuid>,
    question: &str,
    suggestion: &str,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO generated_suggestions
            (id, session_id, exchange_id, question, suggestion, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id.to_string(),
            session_id.to_string(),
            exchange_id.map(|e| e.to_string()),
            question,
            suggestion,
            created_at_ms,
        ],
    )
    .map_err(|e| format!("failed to insert generated suggestion: {e}"))?;
    Ok(id)
}
