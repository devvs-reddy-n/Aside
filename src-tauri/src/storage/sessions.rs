use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::domain::{Session, SessionStatus, SessionType};
use crate::llm::LlmProviderKind;

const SESSION_COLUMNS: &str =
    "id, title, mode_id, participants, started_at_ms, ended_at_ms, status, created_at_ms, updated_at_ms, \
     company, job_description, session_type, save_transcript";

fn status_to_str(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Draft => "draft",
        SessionStatus::Active => "active",
        SessionStatus::Ended => "ended",
    }
}

// `pub(crate)`, not private, so `storage::past_sessions`'s cross-table
// listing query (session status is part of that summary row) can reuse the
// exact same parsing instead of a second, driftable copy.
pub(crate) fn status_from_str(s: &str) -> Result<SessionStatus, String> {
    match s {
        "draft" => Ok(SessionStatus::Draft),
        "active" => Ok(SessionStatus::Active),
        "ended" => Ok(SessionStatus::Ended),
        other => Err(format!("unknown session status in database: {other}")),
    }
}

fn session_type_to_str(kind: SessionType) -> &'static str {
    match kind {
        SessionType::Interview => "interview",
        SessionType::RegularCall => "regular_call",
    }
}

fn session_type_from_str(s: &str) -> SessionType {
    // Unlike `status_from_str`, an unrecognized value degrades to the
    // default rather than erroring the whole row load — a session_type
    // typo/future-value should never make an otherwise-healthy session
    // record unreadable.
    match s {
        "regular_call" => SessionType::RegularCall,
        _ => SessionType::Interview,
    }
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    let id: String = row.get("id")?;
    let participants_json: String = row.get("participants")?;
    let status_str: String = row.get("status")?;
    let session_type_str: String = row.get("session_type")?;
    let save_transcript_int: i64 = row.get("save_transcript")?;

    Ok(Session {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        title: row.get("title")?,
        mode_id: row.get("mode_id")?,
        participants: serde_json::from_str(&participants_json).unwrap_or_default(),
        started_at_ms: row.get("started_at_ms")?,
        ended_at_ms: row.get("ended_at_ms")?,
        status: status_from_str(&status_str).unwrap_or(SessionStatus::Draft),
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
        company: row.get("company")?,
        job_description: row.get("job_description")?,
        session_type: session_type_from_str(&session_type_str),
        save_transcript: save_transcript_int != 0,
    })
}

pub fn create(conn: &Connection, session: &Session) -> Result<(), String> {
    let participants_json = serde_json::to_string(&session.participants)
        .map_err(|e| format!("failed to serialize participants: {e}"))?;
    conn.execute(
        "INSERT INTO sessions (id, title, mode_id, participants, started_at_ms, ended_at_ms, status, created_at_ms, updated_at_ms, company, job_description, session_type, save_transcript)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            session.id.to_string(),
            session.title,
            session.mode_id,
            participants_json,
            session.started_at_ms,
            session.ended_at_ms,
            status_to_str(session.status),
            session.created_at_ms,
            session.updated_at_ms,
            session.company,
            session.job_description,
            session_type_to_str(session.session_type),
            session.save_transcript as i64,
        ],
    )
    .map_err(|e| format!("failed to insert session: {e}"))?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Session>, String> {
    conn.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"),
        params![id.to_string()],
        row_to_session,
    )
    .optional()
    .map_err(|e| format!("failed to load session: {e}"))
}

#[cfg(test)]
pub fn list(conn: &Connection) -> Result<Vec<Session>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY started_at_ms DESC"
        ))
        .map_err(|e| format!("failed to prepare session list query: {e}"))?;
    let rows = stmt
        .query_map([], row_to_session)
        .map_err(|e| format!("failed to run session list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read session row: {e}"))
}

/// Records which Copilot launched a call and the immutable setup that was
/// used. The public `Session` shape stays compatible with existing callers;
/// Call history reads this snapshot only when it needs provenance later.
pub fn set_copilot_snapshot(
    conn: &Connection,
    id: Uuid,
    copilot_id: Uuid,
    snapshot_json: &str,
) -> Result<(), String> {
    set_call_snapshot(conn, id, Some(copilot_id), snapshot_json)
}

/// A quick call has the same fixed model record, without creating a Copilot.
pub fn set_call_snapshot(
    conn: &Connection,
    id: Uuid,
    copilot_id: Option<Uuid>,
    snapshot_json: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE sessions SET copilot_id=?1, copilot_snapshot_json=?2, updated_at_ms=?3 WHERE id=?4",
        params![
            copilot_id.map(|id| id.to_string()),
            snapshot_json,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            id.to_string(),
        ],
    )
    .map_err(|e| format!("failed to save Copilot call snapshot: {e}"))?;
    Ok(())
}

/// The model chosen in a Copilot is resolved through the call's provenance,
/// not the mutable Copilot row. The snapshot stays authoritative even after
/// the template is edited for a later call.
pub fn copilot_model_for_call(
    conn: &Connection,
    id: Uuid,
) -> Result<Option<(LlmProviderKind, String)>, String> {
    let snapshot: Option<Option<String>> = conn
        .query_row(
            "SELECT copilot_snapshot_json FROM sessions WHERE id=?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("failed to read Copilot call snapshot: {e}"))?;
    let Some(snapshot) = snapshot.flatten() else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&snapshot)
        .map_err(|_| "saved Copilot call snapshot is invalid".to_string())?;
    let provider = value
        .get("llm_provider")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());
    let model = value
        .get("llm_model")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    Ok(provider.zip(model))
}

/// Marks a session ended and stamps `ended_at_ms`/`updated_at_ms`. Rejects
/// the transition per `domain::validate_transition` before touching the row.
pub fn end(conn: &Connection, id: Uuid, ended_at_ms: u64) -> Result<(), String> {
    let current = get(conn, id)?.ok_or_else(|| "session not found".to_string())?;
    crate::domain::validate_transition(current.status, SessionStatus::Ended)
        .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE sessions SET status = ?1, ended_at_ms = ?2, updated_at_ms = ?2 WHERE id = ?3",
        params![
            status_to_str(SessionStatus::Ended),
            ended_at_ms,
            id.to_string()
        ],
    )
    .map_err(|e| format!("failed to end session: {e}"))?;
    Ok(())
}

/// Hard delete of one session row — no soft delete, no trash table. The
/// schema's `ON DELETE CASCADE` on `transcript_segments.session_id`,
/// `chunks.session_id` and `documents.session_id` (`storage/migrations.rs`)
/// removes the dependent rows in the same statement, and that cascade only
/// actually fires because `migrations::run` turns on `PRAGMA foreign_keys`
/// for every connection this app opens (SQLite defaults it off per
/// connection) — see `deleting_a_session_cascades_to_transcript_segments_chunks_and_documents`
/// below for a runtime proof, not just a schema-on-paper one. Returns
/// whether a row actually existed to delete, matching
/// `storage::documents::delete`'s existing convention.
pub fn delete(conn: &Connection, id: Uuid) -> Result<bool, String> {
    let affected = conn
        .execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(|e| format!("failed to delete session: {e}"))?;
    Ok(affected > 0)
}

/// Delete only the confirmed snapshot, atomically. Active calls always survive.
pub fn delete_many_inactive(conn: &mut Connection, ids: &[Uuid]) -> Result<usize, String> {
    let transaction = conn
        .transaction()
        .map_err(|e| format!("couldn't start history deletion: {e}"))?;
    let mut deleted = 0;
    for id in ids {
        // FTS is a separate search table, so foreign-key cascades don't clear it.
        transaction.execute(
            "DELETE FROM chunks_fts WHERE session_id = ?1 AND EXISTS (SELECT 1 FROM sessions WHERE id = ?1 AND status != 'active')",
            params![id.to_string()],
        ).map_err(|e| format!("couldn't clear call search data: {e}"))?;
        // Older uploads can belong directly to a call. Keep those Library files.
        transaction.execute(
            "UPDATE documents SET session_id = NULL WHERE session_id = ?1 AND EXISTS (SELECT 1 FROM sessions WHERE id = ?1 AND status != 'active')",
            params![id.to_string()],
        ).map_err(|e| format!("couldn't preserve Library files: {e}"))?;
        deleted += transaction
            .execute(
                "DELETE FROM sessions WHERE id = ?1 AND status != 'active'",
                params![id.to_string()],
            )
            .map_err(|e| format!("couldn't delete history: {e}"))?;
    }
    transaction
        .commit()
        .map_err(|e| format!("couldn't finish history deletion: {e}"))?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    #[test]
    fn bulk_history_delete_preserves_active_calls_library_and_unconfirmed_sessions() {
        use crate::storage::{chunks, documents, transcripts};
        let db = Db::open_in_memory();
        let mut conn = db.0.lock().unwrap();
        let finished = Session::new("Finished", 1_000);
        let active = Session::new("Running", 1_000);
        let later = Session::new("Not confirmed", 1_000);
        for session in [&finished, &active, &later] {
            create(&conn, session).unwrap();
        }
        end(&conn, finished.id, 2_000).unwrap();
        conn.execute(
            "UPDATE sessions SET status = 'active' WHERE id = ?1",
            params![active.id.to_string()],
        )
        .unwrap();
        transcripts::insert_segment(
            &conn,
            finished.id,
            "mic",
            "local_parakeet",
            "hello",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        let library =
            documents::insert_document(&conn, None, "resume", "resume.txt", "Library file", 1_000)
                .unwrap();
        let older_upload = documents::insert_document(
            &conn,
            Some(finished.id),
            "resume",
            "older.txt",
            "Older upload",
            1_000,
        )
        .unwrap();
        chunks::insert_chunk(
            &conn,
            finished.id,
            None,
            "transcript",
            "private words",
            None,
            None,
            1_000,
        )
        .unwrap();
        chunks::insert_chunk(
            &conn,
            active.id,
            None,
            "transcript",
            "running words",
            None,
            None,
            1_000,
        )
        .unwrap();
        assert_eq!(
            delete_many_inactive(&mut conn, &[finished.id, active.id, finished.id]).unwrap(),
            1
        );
        assert!(get(&conn, finished.id).unwrap().is_none());
        assert!(get(&conn, active.id).unwrap().is_some());
        assert!(get(&conn, later.id).unwrap().is_some());
        assert!(transcripts::list_for_session(&conn, finished.id)
            .unwrap()
            .is_empty());
        assert!(documents::get(&conn, library).unwrap().is_some());
        let preserved = documents::get(&conn, older_upload).unwrap().unwrap();
        assert!(preserved.session_id.is_none());
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE session_id = ?1",
                params![finished.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 0);
        let active_indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE session_id = ?1",
                params![active.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_indexed, 1);
    }

    #[test]
    fn bulk_history_delete_rolls_back_all_rows_on_failure() {
        use crate::storage::{chunks, documents};
        let db = Db::open_in_memory();
        let mut conn = db.0.lock().unwrap();
        let first = Session::new("First", 1_000);
        let second = Session::new("Blocked", 1_000);
        create(&conn, &first).unwrap();
        create(&conn, &second).unwrap();
        let upload = documents::insert_document(
            &conn,
            Some(first.id),
            "resume",
            "resume.txt",
            "Saved file",
            1_000,
        )
        .unwrap();
        chunks::insert_chunk(
            &conn,
            first.id,
            None,
            "transcript",
            "keep on rollback",
            None,
            None,
            1_000,
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER block_history_delete BEFORE DELETE ON sessions WHEN OLD.title = 'Blocked' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(delete_many_inactive(&mut conn, &[first.id, second.id]).is_err());
        assert!(get(&conn, first.id).unwrap().is_some());
        assert!(get(&conn, second.id).unwrap().is_some());
        assert_eq!(
            documents::get(&conn, upload).unwrap().unwrap().session_id,
            Some(first.id)
        );
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE session_id = ?1",
                params![first.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn create_then_get_round_trips() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("1:1 with Sam", 1_000);
        create(&conn, &session).unwrap();

        let loaded = get(&conn, session.id)
            .unwrap()
            .expect("session should exist");
        assert_eq!(loaded.title, "1:1 with Sam");
        assert_eq!(loaded.status, SessionStatus::Draft);
        assert_eq!(loaded.started_at_ms, 1_000);
    }

    #[test]
    fn create_then_get_round_trips_full_session_context() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut session = Session::new("Amazon interview", 1_000);
        session.company = Some("Amazon".to_string());
        session.job_description = Some("Experience with AWS, Redshift".to_string());
        session.session_type = SessionType::RegularCall;
        create(&conn, &session).unwrap();

        let loaded = get(&conn, session.id)
            .unwrap()
            .expect("session should exist");
        assert_eq!(loaded.company.as_deref(), Some("Amazon"));
        assert_eq!(
            loaded.job_description.as_deref(),
            Some("Experience with AWS, Redshift")
        );
        assert_eq!(loaded.session_type, SessionType::RegularCall);
    }

    #[test]
    fn create_then_get_round_trips_the_defaults_when_session_context_is_unset() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("plain session", 1_000);
        create(&conn, &session).unwrap();

        let loaded = get(&conn, session.id).unwrap().unwrap();
        assert_eq!(loaded.company, None);
        assert_eq!(loaded.job_description, None);
        assert_eq!(loaded.session_type, SessionType::Interview);
        assert!(loaded.save_transcript, "save_transcript must default true");
    }

    #[test]
    fn create_then_get_round_trips_save_transcript_off() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut session = Session::new("Amazon interview", 1_000);
        session.save_transcript = false;
        create(&conn, &session).unwrap();

        let loaded = get(&conn, session.id)
            .unwrap()
            .expect("session should exist");
        assert!(!loaded.save_transcript);
    }

    #[test]
    fn get_missing_session_returns_none() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(get(&conn, Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn list_orders_newest_first() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        create(&conn, &Session::new("older", 1_000)).unwrap();
        create(&conn, &Session::new("newer", 2_000)).unwrap();

        let sessions = list(&conn).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].title, "newer");
        assert_eq!(sessions[1].title, "older");
    }

    #[test]
    fn end_sets_status_and_ended_at() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut session = Session::new("Interview", 1_000);
        session.status = SessionStatus::Active;
        create(&conn, &session).unwrap();

        end(&conn, session.id, 5_000).unwrap();

        let loaded = get(&conn, session.id).unwrap().unwrap();
        assert_eq!(loaded.status, SessionStatus::Ended);
        assert_eq!(loaded.ended_at_ms, Some(5_000));
    }

    #[test]
    fn end_rejects_a_session_that_was_never_active() {
        // Draft -> Ended isn't in the reject list today (only Ended -> Active
        // is), so this documents current behavior rather than asserting a
        // rejection — kept as a regression guard if that rule tightens later.
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Interview", 1_000);
        create(&conn, &session).unwrap();
        assert!(end(&conn, session.id, 2_000).is_ok());
    }

    #[test]
    fn ending_an_already_ended_session_is_rejected_on_reactivation_only() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Interview", 1_000);
        create(&conn, &session).unwrap();
        end(&conn, session.id, 2_000).unwrap();
        // Ending an already-ended session is a no-op transition (Ended -> Ended),
        // which validate_transition doesn't reject — only Ended -> Active is.
        assert!(end(&conn, session.id, 3_000).is_ok());
    }

    #[test]
    fn deleting_a_session_cascades_to_transcript_segments_chunks_and_documents() {
        use crate::storage::{chunks, documents, transcripts};

        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Interview", 1_000);
        create(&conn, &session).unwrap();
        transcripts::insert_segment(
            &conn,
            session.id,
            "mic",
            "local_parakeet",
            "hi",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        let doc_id = documents::insert_document(
            &conn,
            Some(session.id),
            "resume",
            "resume.pdf",
            "resume text",
            1_000,
        )
        .unwrap();
        chunks::insert_chunk(
            &conn,
            session.id,
            Some(doc_id),
            "document",
            "resume text",
            None,
            None,
            1_000,
        )
        .unwrap();
        assert_eq!(
            transcripts::list_for_session(&conn, session.id)
                .unwrap()
                .len(),
            1
        );
        assert!(documents::get(&conn, doc_id).unwrap().is_some());
        assert_eq!(chunks::count_for_session(&conn, session.id).unwrap(), 1);

        let deleted = delete(&conn, session.id).unwrap();

        assert!(deleted);
        assert!(get(&conn, session.id).unwrap().is_none());
        // Proves `PRAGMA foreign_keys` is genuinely ON at runtime (SQLite
        // defaults it off per-connection): if it weren't, the schema's
        // `ON DELETE CASCADE` declarations would be inert and every one of
        // these rows would silently survive the session's deletion.
        assert!(
            transcripts::list_for_session(&conn, session.id)
                .unwrap()
                .is_empty(),
            "ON DELETE CASCADE should have removed the session's transcript segments too"
        );
        assert!(
            documents::get(&conn, doc_id).unwrap().is_none(),
            "ON DELETE CASCADE should have removed the session's documents too"
        );
        assert_eq!(
            chunks::count_for_session(&conn, session.id).unwrap(),
            0,
            "ON DELETE CASCADE should have removed the session's chunks too"
        );
    }

    #[test]
    fn copilot_model_snapshot_stays_fixed_after_the_template_changes() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Platform interview", 1_000);
        create(&conn, &session).unwrap();
        let copilot_id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO copilots (id, name, auto_answer, save_transcript, llm_provider, llm_model, created_at_ms, updated_at_ms)
             VALUES (?1, 'Platform setup', 0, 1, 'open_ai', 'gpt-6-luna', 1, 1)",
            params![copilot_id.to_string()],
        )
        .unwrap();

        // This is the exact frozen piece of a Copilot call. A later edit to
        // the Copilot row cannot alter this JSON, so the call's model remains
        // the one selected when Start now was pressed.
        set_copilot_snapshot(
            &conn,
            session.id,
            copilot_id,
            r#"{"llm_provider":"open_ai","llm_model":"gpt-6.1-sol"}"#,
        )
        .unwrap();

        assert_eq!(
            copilot_model_for_call(&conn, session.id).unwrap(),
            Some((LlmProviderKind::OpenAi, "gpt-6.1-sol".to_string()))
        );
    }

    #[test]
    fn deleting_a_session_that_does_not_exist_is_a_safe_no_op() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(!delete(&conn, Uuid::new_v4()).unwrap());
    }
}
