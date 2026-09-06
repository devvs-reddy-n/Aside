//! Durable transcript segments (`status.md` Phase 5.1).
//!
//! Mirrors `sessions.rs`'s established shape: plain functions over a
//! `&Connection`, a row-mapping helper, `String`-formatted errors at the
//! storage boundary. One difference from `sessions.rs`: there is no shared
//! `domain::TranscriptSegment` type to build on (unlike `Session`), so this
//! module defines its own `TranscriptSegment` return type — the schema
//! (`storage/migrations.rs`'s `transcript_segments` table) is this module's
//! only real contract.
//!
//! **What actually gets written here**, per `docs/decisions.md`'s "Final
//! transcripts are complete utterances, never deltas": only finals. Nothing
//! in this module enforces that on its own — the schema's `is_final` column
//! genuinely allows storing a non-final row, and `insert_segment` will do it
//! if asked, because a storage accessor shouldn't quietly reinterpret its
//! caller's input. The finals-only rule is enforced one layer up, in
//! `pipeline.rs`'s `persist_finals`, which is the only real caller and is
//! itself tested for exactly this.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

/// One row of `transcript_segments`, as read back from the database.
///
/// Layer 3's Turn table is the first frontend consumer (`list_session_transcripts`).
/// `pipeline.rs` is still the only writer. `get` remains test/future lookup
/// (`source_segment_id`); `list_for_session` is the inspector's reload path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSegment {
    pub id: Uuid,
    pub session_id: Uuid,
    pub channel: String,
    pub origin: String,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: Option<f64>,
    pub is_final: bool,
    pub created_at_ms: u64,
}

fn row_to_segment(row: &rusqlite::Row) -> rusqlite::Result<TranscriptSegment> {
    let id: String = row.get("id")?;
    let session_id: String = row.get("session_id")?;
    let is_final: i64 = row.get("is_final")?;
    Ok(TranscriptSegment {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&session_id).unwrap_or_else(|_| Uuid::nil()),
        channel: row.get("channel")?,
        origin: row.get("origin")?,
        text: row.get("text")?,
        start_ms: row.get("start_ms")?,
        end_ms: row.get("end_ms")?,
        confidence: row.get("confidence")?,
        is_final: is_final != 0,
        created_at_ms: row.get("created_at_ms")?,
    })
}

/// Inserts one segment and returns its generated id.
///
/// `channel` and `origin` are plain strings rather than `Channel`/`EngineKind`
/// on purpose — this module is the storage boundary and shouldn't need to
/// depend on `audio::channel` or `stt` just to accept a tag; the schema's
/// `CHECK` constraint is the real validation, and a bad value fails loudly as
/// a database error rather than silently. Callers (`pipeline.rs`) are
/// expected to pass the exact strings those types already define
/// (`Channel`'s serde spelling, `EngineKind::origin_db_value`).
#[allow(clippy::too_many_arguments)]
pub fn insert_segment(
    conn: &Connection,
    session_id: Uuid,
    channel: &str,
    origin: &str,
    text: &str,
    start_ms: u64,
    end_ms: u64,
    confidence: Option<f64>,
    is_final: bool,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO transcript_segments
            (id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id.to_string(),
            session_id.to_string(),
            channel,
            origin,
            text,
            start_ms,
            end_ms,
            confidence,
            is_final as i64,
            created_at_ms,
        ],
    )
    .map_err(|e| format!("failed to insert transcript segment: {e}"))?;
    Ok(id)
}

/// All segments for one session, in the order they landed (`created_at_ms`).
/// Do not sort by `start_ms`: that clock is a per-channel cursor and puts
/// last-spoken lines in the middle of the notebook.
#[cfg(test)]
pub fn list_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<TranscriptSegment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
             FROM transcript_segments WHERE session_id = ?1
             ORDER BY created_at_ms ASC, end_ms ASC, id ASC",
        )
        .map_err(|e| format!("failed to prepare transcript segment list query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_segment)
        .map_err(|e| format!("failed to run transcript segment list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read transcript segment row: {e}"))
}

/// Every **final** segment for one session, in speech time order
/// (`start_ms`) rather than `list_for_session`'s receipt order — the Past
/// Sessions transcript view (`commands::past_sessions::get_session_transcript`)
/// wants "what was actually said, in the order it was said", not the order
/// each channel's line happened to land in this app.
pub fn list_final_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<TranscriptSegment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
             FROM transcript_segments WHERE session_id = ?1 AND is_final = 1
             ORDER BY start_ms ASC, end_ms ASC, id ASC",
        )
        .map_err(|e| format!("failed to prepare final transcript segment list query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_segment)
        .map_err(|e| format!("failed to run final transcript segment list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read final transcript segment row: {e}"))
}

/// The most recently *received* row for a session (receipt clock), not the
/// oldest-by-`start_ms` list order. Used to decide whether a new final should
/// extend the open notebook line.
pub fn last_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Option<TranscriptSegment>, String> {
    conn.query_row(
        "SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
         FROM transcript_segments WHERE session_id = ?1
         ORDER BY created_at_ms DESC, rowid DESC LIMIT 1",
        params![session_id.to_string()],
        row_to_segment,
    )
    .optional()
    .map_err(|e| format!("failed to load latest transcript segment: {e}"))
}

/// Newest row on one ear for this session (receipt clock).
pub fn last_for_channel(
    conn: &Connection,
    session_id: Uuid,
    channel: &str,
) -> Result<Option<TranscriptSegment>, String> {
    conn.query_row(
        "SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
         FROM transcript_segments WHERE session_id = ?1 AND channel = ?2
         ORDER BY created_at_ms DESC, rowid DESC LIMIT 1",
        params![session_id.to_string(), channel],
        row_to_segment,
    )
    .optional()
    .map_err(|e| format!("failed to load latest {channel} transcript segment: {e}"))
}

pub fn delete_by_id(conn: &Connection, id: Uuid) -> Result<(), String> {
    let n = conn
        .execute(
            "DELETE FROM transcript_segments WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(|e| format!("failed to delete transcript segment: {e}"))?;
    if n == 0 {
        return Err("transcript segment to delete was not found".to_string());
    }
    Ok(())
}

pub fn update_segment_text(
    conn: &Connection,
    id: Uuid,
    text: &str,
    end_ms: u64,
    created_at_ms: u64,
) -> Result<(), String> {
    let n = conn
        .execute(
            "UPDATE transcript_segments SET text = ?1, end_ms = ?2, created_at_ms = ?3 WHERE id = ?4",
            params![text, end_ms, created_at_ms, id.to_string()],
        )
        .map_err(|e| format!("failed to update transcript segment: {e}"))?;
    if n == 0 {
        return Err("transcript segment to update was not found".to_string());
    }
    Ok(())
}

/// Fetches a single segment by id — mainly useful for tests and any future
/// caller (e.g. Phase 7's fact extraction, which references
/// `source_segment_id`) that needs to look one up directly rather than
/// scanning a whole session's list.
#[allow(dead_code)] // real query API for a future consumer — see the struct doc comment above
pub fn get(conn: &Connection, id: Uuid) -> Result<Option<TranscriptSegment>, String> {
    conn.query_row(
        "SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
         FROM transcript_segments WHERE id = ?1",
        params![id.to_string()],
        row_to_segment,
    )
    .optional()
    .map_err(|e| format!("failed to load transcript segment: {e}"))
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

    #[test]
    fn insert_then_get_round_trips_every_field() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let id = insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "hello there",
            1_000,
            2_500,
            Some(0.92),
            true,
            2_500,
        )
        .unwrap();

        let loaded = get(&conn, id).unwrap().expect("segment should exist");
        assert_eq!(loaded.session_id, session_id);
        assert_eq!(loaded.channel, "mic");
        assert_eq!(loaded.origin, "local_parakeet");
        assert_eq!(loaded.text, "hello there");
        assert_eq!(loaded.start_ms, 1_000);
        assert_eq!(loaded.end_ms, 2_500);
        assert_eq!(loaded.confidence, Some(0.92));
        assert!(loaded.is_final);
        assert_eq!(loaded.created_at_ms, 2_500);
    }

    #[test]
    fn get_missing_segment_returns_none() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(get(&conn, Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn channel_and_origin_tagging_is_preserved_per_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let mic_id = insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "you: hi",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        let system_id = insert_segment(
            &conn,
            session_id,
            "system",
            "deepgram",
            "them: hi back",
            100,
            200,
            None,
            true,
            200,
        )
        .unwrap();

        let mic = get(&conn, mic_id).unwrap().unwrap();
        let system = get(&conn, system_id).unwrap().unwrap();
        assert_eq!(mic.channel, "mic");
        assert_eq!(mic.origin, "local_parakeet");
        assert_eq!(system.channel, "system");
        assert_eq!(system.origin, "deepgram");
    }

    #[test]
    fn list_for_session_orders_by_when_the_line_landed() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "third",
            3_000,
            3_500,
            None,
            true,
            3_500,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "first",
            1_000,
            1_500,
            None,
            true,
            1_500,
        )
        .unwrap();
        insert_segment(
            &conn, session_id, "system", "deepgram", "second", 2_000, 2_500, None, true, 2_500,
        )
        .unwrap();

        let segments = list_for_session(&conn, session_id).unwrap();
        let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn a_late_line_with_an_early_start_ms_still_sorts_last() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_segment(
            &conn,
            session_id,
            "system",
            "speechmatics",
            "others first",
            5_000,
            5_100,
            None,
            true,
            2_000,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "me spoke last",
            100,
            9_000,
            None,
            true,
            9_000,
        )
        .unwrap();
        let segments = list_for_session(&conn, session_id).unwrap();
        let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["others first", "me spoke last"]);
    }

    #[test]
    fn sessions_do_not_leak_segments_into_each_others_lists() {
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_segment(
            &conn,
            session_a,
            "mic",
            "local_parakeet",
            "a's line",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_b,
            "mic",
            "local_parakeet",
            "b's line",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();

        let a_segments = list_for_session(&conn, session_a).unwrap();
        let b_segments = list_for_session(&conn, session_b).unwrap();
        assert_eq!(a_segments.len(), 1);
        assert_eq!(a_segments[0].text, "a's line");
        assert_eq!(b_segments.len(), 1);
        assert_eq!(b_segments[0].text, "b's line");
    }

    #[test]
    fn list_for_session_with_no_segments_is_an_empty_vec_not_an_error() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        assert!(list_for_session(&conn, session_id).unwrap().is_empty());
    }

    #[test]
    fn a_transcript_segment_survives_closing_and_reopening_the_database_file() {
        // Mirrors `storage/mod.rs`'s `a_session_survives_closing_and_reopening_the_database_file`
        // — the equivalent proof for `transcript_segments`, closing 5.2's stated gap that
        // this table had only ever been round-tripped against an in-memory database.
        let path = std::env::temp_dir().join(format!("aside-test-{}.sqlite3", Uuid::new_v4()));

        let (session_id, segment_id) = {
            let db = Db::open(&path).expect("first open");
            let conn = db.0.lock().unwrap();
            let session = Session::new("Transcript persistence test", 1_000);
            sessions::create(&conn, &session).unwrap();
            let segment_id = insert_segment(
                &conn,
                session.id,
                "mic",
                "local_parakeet",
                "this should survive a restart",
                1_000,
                2_500,
                Some(0.87),
                true,
                2_500,
            )
            .unwrap();
            (session.id, segment_id)
            // `db` (and its Connection) drops here — simulates the app quitting.
        };

        let reloaded = {
            let db = Db::open(&path).expect("second open");
            let conn = db.0.lock().unwrap();
            get(&conn, segment_id).unwrap()
        };
        let reloaded_list = {
            let db = Db::open(&path).expect("third open, for list_for_session");
            let conn = db.0.lock().unwrap();
            list_for_session(&conn, session_id).unwrap()
        };

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));

        let reloaded = reloaded.expect("segment should have persisted");
        assert_eq!(reloaded.text, "this should survive a restart");
        assert_eq!(reloaded.session_id, session_id);
        assert_eq!(reloaded.confidence, Some(0.87));
        assert_eq!(reloaded_list.len(), 1);
        assert_eq!(reloaded_list[0].id, segment_id);
    }

    #[test]
    fn is_final_false_is_still_stored_faithfully_the_finals_only_rule_lives_in_pipeline_rs() {
        // This module is a pure storage accessor: it stores whatever it's
        // told, including a non-final row. The house rule that only finals
        // ever get here is enforced by `pipeline.rs`'s `persist_finals`, not
        // by this insert function — covered separately in `pipeline.rs`'s
        // own tests.
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        let id = insert_segment(
            &conn,
            session_id,
            "mic",
            "local_parakeet",
            "still going",
            0,
            50,
            None,
            false,
            50,
        )
        .unwrap();
        assert!(!get(&conn, id).unwrap().unwrap().is_final);
    }

    #[test]
    fn listed_segments_serialize_with_snake_case_channel_tags() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "My current work authorization is STEM OPT.",
            1_000,
            1_800,
            None,
            true,
            1_800,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "system",
            "speechmatics",
            "Can you explain your Kubernetes experience?",
            2_000,
            2_600,
            None,
            true,
            2_600,
        )
        .unwrap();

        let rows = list_for_session(&conn, session_id).unwrap();
        let json =
            serde_json::to_value(&rows).expect("TranscriptSegment must be JSON-serializable");
        assert!(json[0].get("session_id").is_some());
        assert!(json[0].get("sessionId").is_none());
        assert_eq!(json[0]["channel"], "mic");
        assert_eq!(json[1]["channel"], "system");
        assert_eq!(json[0]["is_final"], true);
        assert!(json[0]["text"].as_str().unwrap().contains("STEM OPT"));
        assert!(
            !json[0]["text"].as_str().unwrap().contains("Kubernetes"),
            "mic row must not carry Others text"
        );
        assert!(json[1]["text"].as_str().unwrap().contains("Kubernetes"));
        assert_eq!(json[0]["origin"], "speechmatics");
    }

    #[test]
    fn list_final_for_session_returns_only_finals_in_speech_time_order() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        // Received out of speech order (mirrors `a_late_line_with_an_early_start_ms_still_sorts_last`'s
        // setup) so this test actually proves start_ms ordering, not just receipt-order luck.
        insert_segment(
            &conn,
            session_id,
            "system",
            "speechmatics",
            "second spoken",
            2_000,
            2_500,
            None,
            true,
            500,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "first spoken",
            1_000,
            1_500,
            None,
            true,
            9_000,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "still typing",
            3_000,
            3_100,
            None,
            false,
            3_100,
        )
        .unwrap();

        let finals = list_final_for_session(&conn, session_id).unwrap();
        let texts: Vec<&str> = finals.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["first spoken", "second spoken"],
            "non-final row must be excluded"
        );
        assert!(finals.iter().all(|s| s.is_final));
    }

    #[test]
    fn last_for_channel_then_delete_removes_only_that_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "keep me",
            1,
            2,
            None,
            true,
            1,
        )
        .unwrap();
        let echo_id = insert_segment(
            &conn,
            session_id,
            "mic",
            "speechmatics",
            "echo of others",
            3,
            4,
            None,
            true,
            3,
        )
        .unwrap();
        insert_segment(
            &conn,
            session_id,
            "system",
            "speechmatics",
            "others",
            5,
            6,
            None,
            true,
            5,
        )
        .unwrap();
        let latest_mic = last_for_channel(&conn, session_id, "mic").unwrap().unwrap();
        assert_eq!(latest_mic.id, echo_id);
        delete_by_id(&conn, echo_id).unwrap();
        let leftover = last_for_channel(&conn, session_id, "mic").unwrap().unwrap();
        assert_eq!(leftover.text, "keep me");
        assert_eq!(list_for_session(&conn, session_id).unwrap().len(), 2);
    }
}
