//! Extracted facts (visa status, notice period, title, location — `status.md`
//! 7.9). Follows `storage/transcripts.rs`'s established shape (plain
//! functions over a `&Connection`, a row-mapping helper, `String`-formatted
//! errors), with one real difference from `transcripts.rs`'s pure
//! insert-only shape: **a correction replaces the existing value for the
//! same key, it never appends a second row.** The schema already enforces
//! this — `migrations.rs`'s `facts` table has `UNIQUE (session_id, key)` —
//! so `upsert_fact` is a real `ON CONFLICT` upsert, matching
//! `storage/settings.rs`'s own upsert shape, not a plain `INSERT`.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub id: Uuid,
    pub session_id: Uuid,
    pub key: String,
    pub value: String,
    pub source: String,
    pub priority: i64,
    pub source_segment_id: Option<Uuid>,
    pub confidence: Option<f64>,
    pub updated_at_ms: u64,
}

pub const SOURCE_SPOKEN: &str = "spoken_correction";
pub const SOURCE_DOCUMENT: &str = "document";
#[allow(dead_code)]
pub const SOURCE_PREVIOUS: &str = "previous_session";
pub const SOURCE_REGEX: &str = "regex";

pub const PRIORITY_SPOKEN: i64 = 80;
pub const PRIORITY_DOCUMENT: i64 = 50;
#[allow(dead_code)]
pub const PRIORITY_PREVIOUS: i64 = 20;
pub const PRIORITY_REGEX: i64 = 10;

fn row_to_fact(row: &rusqlite::Row) -> rusqlite::Result<Fact> {
    let id: String = row.get("id")?;
    let session_id: String = row.get("session_id")?;
    let source_segment_id: Option<String> = row.get("source_segment_id")?;
    Ok(Fact {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&session_id).unwrap_or_else(|_| Uuid::nil()),
        key: row.get("key")?,
        value: row.get("value")?,
        source: row.get("source")?,
        priority: row.get("priority")?,
        source_segment_id: source_segment_id.and_then(|s| Uuid::parse_str(&s).ok()),
        confidence: row.get("confidence")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}

/// Inserts a new fact, or **replaces** the existing one for the same
/// `(session_id, key)` pair — a later "actually my notice period is one
/// month" must overwrite the earlier "two weeks" row, not sit alongside it
/// as a second, contradictory fact a later prompt could pick either of.
/// Returns the row's id (a fresh one on insert, the existing one preserved
/// on update — `ON CONFLICT ... DO UPDATE` never changes the primary key).
#[cfg(test)]
pub fn upsert_fact(
    conn: &Connection,
    session_id: Uuid,
    key: &str,
    value: &str,
    source_segment_id: Option<Uuid>,
    confidence: Option<f64>,
    updated_at_ms: u64,
) -> Result<Uuid, String> {
    upsert_fact_ranked(
        conn,
        session_id,
        key,
        value,
        SOURCE_REGEX,
        PRIORITY_REGEX,
        source_segment_id,
        confidence,
        updated_at_ms,
    )
}

/// Same UNIQUE(session_id, key) row. A new value replaces the old one only
/// when its priority is **greater or equal**. A weaker source (resume H-1B
/// at 50) cannot overwrite a briefing STEM OPT at 100.
#[allow(clippy::too_many_arguments)]
pub fn upsert_fact_ranked(
    conn: &Connection,
    session_id: Uuid,
    key: &str,
    value: &str,
    source: &str,
    priority: i64,
    source_segment_id: Option<Uuid>,
    confidence: Option<f64>,
    updated_at_ms: u64,
) -> Result<Uuid, String> {
    let new_id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO facts (id, session_id, key, value, source, priority, source_segment_id, confidence, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(session_id, key) DO UPDATE SET
            value = excluded.value,
            source = excluded.source,
            priority = excluded.priority,
            source_segment_id = excluded.source_segment_id,
            confidence = excluded.confidence,
            updated_at_ms = excluded.updated_at_ms
         WHERE excluded.priority >= facts.priority",
        params![
            new_id.to_string(),
            session_id.to_string(),
            key,
            value,
            source,
            priority,
            source_segment_id.map(|id| id.to_string()),
            confidence,
            updated_at_ms,
        ],
    )
    .map_err(|e| format!("failed to upsert fact {key:?}: {e}"))?;

    conn.query_row(
        "SELECT id FROM facts WHERE session_id = ?1 AND key = ?2",
        params![session_id.to_string(), key],
        |row| {
            let id: String = row.get(0)?;
            Ok(Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()))
        },
    )
    .map_err(|e| format!("failed to read back upserted fact {key:?}: {e}"))
}

/// Not yet called outside this module's own tests — real query API for a
/// future prompt-builder consumer, same "land the table's full read API
/// before every consumer exists" precedent `storage::transcripts`'s own
/// `list_for_session` already set.
#[allow(dead_code)]
pub fn list_for_session(conn: &Connection, session_id: Uuid) -> Result<Vec<Fact>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, key, value, source, priority, source_segment_id, confidence, updated_at_ms
             FROM facts WHERE session_id = ?1 ORDER BY priority DESC, key ASC",
        )
        .map_err(|e| format!("failed to prepare fact list query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_fact)
        .map_err(|e| format!("failed to run fact list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read fact row: {e}"))
}

#[allow(dead_code)] // real query API for a future consumer (a single-fact lookup, e.g. a prompt builder)
pub fn get(conn: &Connection, session_id: Uuid, key: &str) -> Result<Option<Fact>, String> {
    conn.query_row(
        "SELECT id, session_id, key, value, source, priority, source_segment_id, confidence, updated_at_ms
         FROM facts WHERE session_id = ?1 AND key = ?2",
        params![session_id.to_string(), key],
        row_to_fact,
    )
    .optional()
    .map_err(|e| format!("failed to load fact {key:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{sessions, Db};

    fn seeded_session(db: &Db) -> Uuid {
        let conn = db.0.lock().unwrap();
        let session = Session::new("Test session", 1_000);
        sessions::create(&conn, &session).unwrap();
        session.id
    }

    /// `facts.source_segment_id` is a real foreign key
    /// (`REFERENCES transcript_segments(id)`, `migrations.rs`'s `run`
    /// enables `PRAGMA foreign_keys = ON`) — a fact can only cite a segment
    /// that actually exists, so tests that want a non-`None` provenance
    /// value must insert a real one first, not fabricate a random `Uuid`.
    fn seeded_segment(db: &Db, session_id: Uuid) -> Uuid {
        use crate::storage::transcripts;
        let conn = db.0.lock().unwrap();
        transcripts::insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "my notice period is two weeks",
            0,
            1_000,
            None,
            true,
            1_000,
        )
        .unwrap()
    }

    #[test]
    fn insert_then_get_round_trips_every_field() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let segment_id = seeded_segment(&db, session_id);
        let conn = db.0.lock().unwrap();

        upsert_fact(
            &conn,
            session_id,
            "notice_period",
            "two weeks",
            Some(segment_id),
            Some(0.9),
            1_000,
        )
        .unwrap();

        let loaded = get(&conn, session_id, "notice_period")
            .unwrap()
            .expect("fact should exist");
        assert_eq!(loaded.value, "two weeks");
        assert_eq!(loaded.source_segment_id, Some(segment_id));
        assert_eq!(loaded.confidence, Some(0.9));
    }

    #[test]
    fn a_correction_replaces_the_value_rather_than_appending_a_second_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        upsert_fact(
            &conn,
            session_id,
            "notice_period",
            "two weeks",
            None,
            None,
            1_000,
        )
        .unwrap();
        upsert_fact(
            &conn,
            session_id,
            "notice_period",
            "one month",
            None,
            None,
            2_000,
        )
        .unwrap();

        let all = list_for_session(&conn, session_id).unwrap();
        assert_eq!(
            all.len(),
            1,
            "a correction must replace, not add a second row for the same key"
        );
        assert_eq!(all[0].value, "one month");
        assert_eq!(all[0].updated_at_ms, 2_000);
    }

    #[test]
    fn different_keys_coexist_independently() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        upsert_fact(
            &conn,
            session_id,
            "notice_period",
            "two weeks",
            None,
            None,
            1_000,
        )
        .unwrap();
        upsert_fact(&conn, session_id, "visa_status", "h1-b", None, None, 1_000).unwrap();
        let all = list_for_session(&conn, session_id).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn the_same_key_in_different_sessions_does_not_collide() {
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        upsert_fact(&conn, session_a, "title", "engineer", None, None, 1_000).unwrap();
        upsert_fact(&conn, session_b, "title", "manager", None, None, 1_000).unwrap();
        assert_eq!(
            get(&conn, session_a, "title").unwrap().unwrap().value,
            "engineer"
        );
        assert_eq!(
            get(&conn, session_b, "title").unwrap().unwrap().value,
            "manager"
        );
    }

    #[test]
    fn missing_fact_returns_none() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        assert!(get(&conn, session_id, "visa_status").unwrap().is_none());
    }

    #[test]
    fn correction_preserves_the_original_row_id() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        let first_id =
            upsert_fact(&conn, session_id, "location", "remote", None, None, 1_000).unwrap();
        let second_id = upsert_fact(
            &conn,
            session_id,
            "location",
            "hybrid, nyc",
            None,
            None,
            2_000,
        )
        .unwrap();
        assert_eq!(
            first_id, second_id,
            "a correction updates the existing row, not a fresh one"
        );
    }

    #[test]
    fn spoken_stem_opt_at_80_beats_a_document_h1b_at_50_and_resists_being_overwritten() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "H-1B",
            SOURCE_DOCUMENT,
            PRIORITY_DOCUMENT,
            None,
            None,
            1_000,
        )
        .unwrap();
        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "STEM OPT",
            SOURCE_SPOKEN,
            PRIORITY_SPOKEN,
            None,
            None,
            2_000,
        )
        .unwrap();
        let loaded = get(&conn, session_id, "visa_status").unwrap().unwrap();
        assert_eq!(loaded.value, "STEM OPT");
        assert_eq!(loaded.priority, PRIORITY_SPOKEN);

        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "H-1B sponsored",
            SOURCE_DOCUMENT,
            PRIORITY_DOCUMENT,
            None,
            None,
            3_000,
        )
        .unwrap();
        let still = get(&conn, session_id, "visa_status").unwrap().unwrap();
        assert_eq!(
            still.value, "STEM OPT",
            "a weaker document fact must not overwrite the spoken correction"
        );
        assert_eq!(still.priority, PRIORITY_SPOKEN);
    }

    #[test]
    fn spoken_stem_opt_at_80_replaces_extracted_opt() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "OPT",
            SOURCE_REGEX,
            PRIORITY_REGEX,
            None,
            None,
            1_000,
        )
        .unwrap();
        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "STEM OPT",
            SOURCE_SPOKEN,
            PRIORITY_SPOKEN,
            None,
            None,
            2_000,
        )
        .unwrap();
        let loaded = get(&conn, session_id, "visa_status").unwrap().unwrap();
        assert_eq!(loaded.value, "STEM OPT");
        assert_eq!(loaded.priority, PRIORITY_SPOKEN);
        assert_eq!(loaded.source, SOURCE_SPOKEN);

        upsert_fact_ranked(
            &conn,
            session_id,
            "visa_status",
            "H-1B",
            SOURCE_DOCUMENT,
            PRIORITY_DOCUMENT,
            None,
            None,
            3_000,
        )
        .unwrap();
        let still = get(&conn, session_id, "visa_status").unwrap().unwrap();
        assert_eq!(
            still.value, "STEM OPT",
            "document 50 must not overwrite spoken 80"
        );
    }
}
