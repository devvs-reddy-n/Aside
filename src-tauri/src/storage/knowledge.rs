//! Heuristic knowledge-pack rows (skills, tech, JD lines, company terms,
//! STT glossary). Not Natively's full graph — a small per-session list.

use rusqlite::{params, Connection};
use uuid::Uuid;

pub const KIND_SKILL: &str = "skill";
pub const KIND_TECH: &str = "tech";
pub const KIND_JD_LINE: &str = "jd_line";
pub const KIND_COMPANY_TERM: &str = "company_term";
pub const KIND_STT_GLOSSARY: &str = "stt_glossary";

/// Speechmatics `additional_vocab` union. A bounded list keeps
/// the extra-vocab payload small on the live socket.
pub const GLOSSARY_CAP: usize = crate::stt::vocabulary::CAP;

pub fn insert(
    conn: &Connection,
    session_id: Uuid,
    kind: &str,
    value: &str,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("knowledge pack value cannot be empty".to_string());
    }
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO knowledge_pack_entries (id, session_id, kind, value, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(session_id, kind, value) DO NOTHING",
        params![
            id.to_string(),
            session_id.to_string(),
            kind,
            value,
            created_at_ms
        ],
    )
    .map_err(|e| format!("failed to insert knowledge pack entry: {e}"))?;
    Ok(id)
}

#[allow(dead_code)]
pub fn list_kind(conn: &Connection, session_id: Uuid, kind: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT value FROM knowledge_pack_entries
             WHERE session_id = ?1 AND kind = ?2
             ORDER BY created_at_ms ASC",
        )
        .map_err(|e| format!("failed to prepare knowledge pack query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string(), kind], |row| row.get(0))
        .map_err(|e| format!("failed to run knowledge pack query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read knowledge pack row: {e}"))
}

/// Speechmatics additional_vocab union, capped.
pub fn glossary(conn: &Connection, session_id: Uuid, cap: usize) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT value FROM knowledge_pack_entries
             WHERE session_id = ?1 AND kind IN ('stt_glossary','tech','company_term')
             ORDER BY CASE kind
                WHEN 'stt_glossary' THEN 0
                WHEN 'company_term' THEN 1
                ELSE 2 END, created_at_ms ASC",
        )
        .map_err(|e| format!("failed to prepare glossary query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| format!("failed to run glossary query: {e}"))?;
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        let v = row.map_err(|e| format!("failed to read glossary row: {e}"))?;
        if out.iter().any(|e| e.eq_ignore_ascii_case(&v)) {
            continue;
        }
        out.push(v);
        if out.len() >= cap {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{sessions, Db};

    #[test]
    fn glossary_caps_and_dedupes() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        insert(&conn, session.id, KIND_STT_GLOSSARY, "Redshift", 1).unwrap();
        insert(&conn, session.id, KIND_TECH, "redshift", 2).unwrap();
        insert(&conn, session.id, KIND_COMPANY_TERM, "Acme", 3).unwrap();
        let g = glossary(&conn, session.id, GLOSSARY_CAP).unwrap();
        assert_eq!(
            g.iter()
                .filter(|v| v.eq_ignore_ascii_case("redshift"))
                .count(),
            1
        );
        assert!(g.iter().any(|v| v == "Acme"));
    }

    #[test]
    fn glossary_truncates_at_the_cap_when_there_are_more_terms() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        for i in 0..(GLOSSARY_CAP + 10) {
            insert(
                &conn,
                session.id,
                KIND_STT_GLOSSARY,
                &format!("UniqueTerm{i:03}"),
                i as u64,
            )
            .unwrap();
        }
        let g = glossary(&conn, session.id, GLOSSARY_CAP).unwrap();
        assert_eq!(g.len(), GLOSSARY_CAP);
        assert_eq!(GLOSSARY_CAP, 64);
    }
}
