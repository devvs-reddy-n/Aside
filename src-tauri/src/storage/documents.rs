//! Uploaded resume/job-description documents (`status.md` 8.5). Follows
//! `storage/transcripts.rs` and `storage/chunks.rs`'s established shape
//! exactly: plain functions over a `&Connection`, a row-mapping helper,
//! `String`-formatted errors at the storage boundary. The schema
//! (`storage/migrations.rs`'s `documents` table) is this module's only real
//! contract — `doc_type` is a plain string validated by the schema's own
//! `CHECK (doc_type IN ('resume','job_description','other'))`, matching
//! `transcripts.rs`'s own reasoning for why `channel`/`origin` are plain
//! strings there too: a storage accessor shouldn't need to import a
//! higher-level type just to accept a tag, and a bad value should fail
//! loudly as a real database error, not be silently coerced.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StoredDocument {
    pub id: Uuid,
    pub session_id: Option<Uuid>,
    pub doc_type: String,
    pub file_name: String,
    pub raw_text: String,
    pub original_available: bool,
    pub mime_type: Option<String>,
    pub byte_size: Option<u64>,
    pub created_at_ms: u64,
}

const DOCUMENT_COLUMNS: &str = "id, session_id, doc_type, file_name, raw_text, original_rel_path, mime_type, byte_size, created_at_ms";

fn row_to_document(row: &rusqlite::Row) -> rusqlite::Result<StoredDocument> {
    let id: String = row.get("id")?;
    let session_id: Option<String> = row.get("session_id")?;
    Ok(StoredDocument {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        session_id: session_id.and_then(|s| Uuid::parse_str(&s).ok()),
        doc_type: row.get("doc_type")?,
        file_name: row.get("file_name")?,
        raw_text: row.get("raw_text")?,
        original_available: row.get::<_, Option<String>>("original_rel_path")?.is_some(),
        mime_type: row.get("mime_type")?,
        byte_size: row
            .get::<_, Option<i64>>("byte_size")?
            .map(|v| v.max(0) as u64),
        created_at_ms: row.get("created_at_ms")?,
    })
}

pub fn set_original_metadata(
    conn: &Connection,
    id: Uuid,
    relative_path: &str,
    mime_type: &str,
    byte_size: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE documents SET original_rel_path=?1, mime_type=?2, byte_size=?3 WHERE id=?4",
        params![relative_path, mime_type, byte_size as i64, id.to_string()],
    )
    .map_err(|e| format!("failed to save original document metadata: {e}"))?;
    Ok(())
}

pub fn original_relative_path(conn: &Connection, id: Uuid) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT original_rel_path FROM documents WHERE id=?1",
        params![id.to_string()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("failed to locate original document: {e}"))
}

/// Inserts one document and returns its generated id. `session_id` is
/// `Option` — the schema allows `NULL` (`ON DELETE CASCADE` against a
/// nullable foreign key) since a resume upload is naturally a once-per-user
/// artifact that can outlive any one session, even though today's only real
/// caller (`commands::documents::ingest_document`) always attaches one.
pub fn insert_document(
    conn: &Connection,
    session_id: Option<Uuid>,
    doc_type: &str,
    file_name: &str,
    raw_text: &str,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO documents (id, session_id, doc_type, file_name, raw_text, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id.to_string(),
            session_id.map(|s| s.to_string()),
            doc_type,
            file_name,
            raw_text,
            created_at_ms
        ],
    )
    .map_err(|e| format!("failed to insert document: {e}"))?;
    Ok(id)
}

/// Inserts `raw_text` as a document, first deleting any existing document
/// that matches on `(session_id, doc_type, file_name)` — the upsert Gap 1's
/// pasted-text fields need. Re-saving an edited paste must REPLACE the
/// previous pasted document of that field, not add a second one alongside
/// it, or every edit would silently grow the prompt forever. The caller
/// supplies a stable marker name per pasted field (e.g. "Pasted job
/// description"); a real uploaded file keeps going through
/// `insert_document` directly and is never touched by this path, since two
/// different real uploads of the same type legitimately coexist. Deleting
/// the previous row also drops its chunks via the schema's `ON DELETE
/// CASCADE` on `chunks.document_id` (`storage/migrations.rs`), so the old
/// paste's search chunks don't linger.
pub fn upsert_document(
    conn: &Connection,
    session_id: Option<Uuid>,
    doc_type: &str,
    file_name: &str,
    raw_text: &str,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    match session_id {
        Some(sid) => conn.execute(
            "DELETE FROM documents WHERE session_id = ?1 AND doc_type = ?2 AND file_name = ?3",
            params![sid.to_string(), doc_type, file_name],
        ),
        None => conn.execute(
            "DELETE FROM documents WHERE session_id IS NULL AND doc_type = ?1 AND file_name = ?2",
            params![doc_type, file_name],
        ),
    }
    .map_err(|e| format!("failed to replace the previous pasted document: {e}"))?;
    insert_document(
        conn,
        session_id,
        doc_type,
        file_name,
        raw_text,
        created_at_ms,
    )
}

/// Deletes one document by id — Gap 2, the page's "remove a wrong upload"
/// case. Chunks referencing it via `document_id` are removed automatically
/// by the schema's `ON DELETE CASCADE` (`storage/migrations.rs`'s `chunks`
/// table), so this does not need to touch `chunks` itself. Returns whether
/// a row actually existed to delete, so a caller can tell "removed" from
/// "already gone" without a separate lookup.
pub fn delete(conn: &Connection, id: Uuid) -> Result<bool, String> {
    let affected = conn
        .execute(
            "DELETE FROM documents WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(|e| format!("failed to delete document: {e}"))?;
    Ok(affected > 0)
}

/// Fetches a single document by id — used by
/// `commands::documents::attach_document_to_session` to load an
/// already-extracted document's text for re-indexing under a new session
/// (2026-08-25 setup-screen work), and by this module's own tests.
pub fn get(conn: &Connection, id: Uuid) -> Result<Option<StoredDocument>, String> {
    conn.query_row(
        &format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE id = ?1"),
        params![id.to_string()],
        row_to_document,
    )
    .optional()
    .map_err(|e| format!("failed to load document: {e}"))
}

/// All documents attached to one session, oldest first — the list a
/// session-history/settings UI would show ("resume uploaded 2 minutes
/// ago"). Real query API for a consumer that doesn't exist yet, same
/// "land the table's full read API before every consumer exists" precedent
/// `storage::transcripts`/`storage::chunks` already set.
#[cfg(test)]
pub fn list_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<StoredDocument>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {DOCUMENT_COLUMNS}
             FROM documents WHERE session_id = ?1 ORDER BY created_at_ms ASC",
        ))
        .map_err(|e| format!("failed to prepare document list query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_document)
        .map_err(|e| format!("failed to run document list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read document row: {e}"))
}

/// All documents that should feed one session's default context: that
/// session's own documents plus the owner's session-less library (`session_id
/// IS NULL`), which behaves like a personal library available to every
/// session — a resume uploaded before any session existed, or before this
/// one, is not orphaned, it is reusable. Session-specific rows are ordered
/// before session-less ones (`(session_id IS NULL)` sorts false-before-true,
/// i.e. 0 before 1 in SQLite), because `load_default_context_from_store`
/// claims the resume/job-description slots on `if ctx.resume.is_none()` —
/// first row wins, so a per-session document must be seen before a library
/// one to actually override it. Within each group, oldest first
/// (`created_at_ms ASC`), matching `list_for_session`'s existing order.
/// Deliberately a sibling of `list_for_session` rather than a change to it:
/// `list_for_session` stays strictly scoped for any future caller (e.g. a
/// "documents in this session" list) that must not see the library.
pub fn list_for_context(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<StoredDocument>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {DOCUMENT_COLUMNS}
             FROM documents
             WHERE session_id = ?1 OR session_id IS NULL
             ORDER BY (session_id IS NULL), created_at_ms ASC"
        ))
        .map_err(|e| format!("failed to prepare document context query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_document)
        .map_err(|e| format!("failed to run document context query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read document row: {e}"))
}

/// The `limit` most recently uploaded documents of `doc_type`, newest
/// first — the setup screen's "pick a previously uploaded resume" dropdown
/// (2026-08-25). Deliberately not scoped to one session (unlike
/// `list_for_session`): a resume is naturally reusable across many
/// sessions, and this is exactly the cross-session query that reuse needs.
pub fn list_recent(
    conn: &Connection,
    doc_type: &str,
    limit: i64,
) -> Result<Vec<StoredDocument>, String> {
    let mut stmt = conn
        .prepare(
            &format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE doc_type = ?1 ORDER BY created_at_ms DESC LIMIT ?2"),
        )
        .map_err(|e| format!("failed to prepare recent document query: {e}"))?;
    let rows = stmt
        .query_map(params![doc_type, limit], row_to_document)
        .map_err(|e| format!("failed to run recent document query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read document row: {e}"))
}

/// Records that `document_id` is the one `session_id` actually uses for
/// `doc_type` in its prompt context — the fix for the "wrong resume
/// answered" bug: `list_for_context`'s oldest-row-wins scan over the
/// session-less library is a fine default, but once the owner has
/// explicitly picked a document (Resume/Documents modal, or a direct
/// in-session upload), that pick must win regardless of upload order.
/// `INSERT OR REPLACE` so re-picking a different document of the same
/// `doc_type` for this session cleanly replaces the previous pick — the
/// `(session_id, document_id)` primary key means picking the *same*
/// document twice is a harmless no-op, and switching to a different one
/// simply adds a second row that `list_attached_for_session` (grouped by
/// `doc_type`, newest `attached_at_ms` first) resolves to "most recently
/// attached wins" for that type.
pub fn attach_to_session(
    conn: &Connection,
    session_id: Uuid,
    document_id: Uuid,
    doc_type: &str,
    attached_at_ms: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO session_documents (session_id, document_id, doc_type, attached_at_ms)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id.to_string(), document_id.to_string(), doc_type, attached_at_ms],
    )
    .map_err(|e| format!("failed to record document attachment: {e}"))?;
    Ok(())
}

/// The documents `session_id` has explicitly attached, one row per
/// `doc_type` (the most recently attached one, when more than one was ever
/// picked) — what `window_memory::load_default_context_from_store` should
/// prefer over `list_for_context`'s oldest-row-wins library scan whenever
/// this returns anything at all. Empty for a session that never went
/// through `attach_to_session` (e.g. an older session, or a document
/// uploaded straight into a live session without an explicit pick), so the
/// caller can fall back to the old scan in exactly that case.
pub fn list_attached_for_session(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<StoredDocument>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT d.id, d.session_id, d.doc_type, d.file_name, d.raw_text, d.original_rel_path, d.mime_type, d.byte_size, d.created_at_ms
             FROM session_documents sd
             JOIN documents d ON d.id = sd.document_id
             WHERE sd.session_id = ?1
             ORDER BY sd.doc_type, sd.attached_at_ms DESC",
        )
        .map_err(|e| format!("failed to prepare attached document query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_document)
        .map_err(|e| format!("failed to run attached document query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read document row: {e}"))
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

        let id = insert_document(
            &conn,
            Some(session_id),
            "resume",
            "jordan_rivera_resume.pdf",
            "Jordan Rivera - Software Engineer. 5 years of Python experience.",
            5_000,
        )
        .unwrap();

        let loaded = get(&conn, id).unwrap().expect("document should exist");
        assert_eq!(loaded.session_id, Some(session_id));
        assert_eq!(loaded.doc_type, "resume");
        assert_eq!(loaded.file_name, "jordan_rivera_resume.pdf");
        assert!(loaded.raw_text.contains("5 years of Python experience"));
        assert_eq!(loaded.created_at_ms, 5_000);
    }

    #[test]
    fn get_missing_document_returns_none() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(get(&conn, Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn a_document_with_no_session_round_trips_with_a_none_session_id() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let id = insert_document(
            &conn,
            None,
            "job_description",
            "role.pdf",
            "We are hiring.",
            1_000,
        )
        .unwrap();
        assert_eq!(get(&conn, id).unwrap().unwrap().session_id, None);
    }

    #[test]
    fn an_invalid_doc_type_is_rejected_by_the_schemas_check_constraint() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let result = insert_document(&conn, None, "not_a_real_type", "x.pdf", "text", 1_000);
        assert!(
            result.is_err(),
            "the schema's CHECK constraint must reject an unknown doc_type"
        );
    }

    #[test]
    fn list_for_session_orders_oldest_first_and_does_not_leak_across_sessions() {
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            Some(session_a),
            "resume",
            "second.pdf",
            "second",
            2_000,
        )
        .unwrap();
        insert_document(
            &conn,
            Some(session_a),
            "resume",
            "first.pdf",
            "first",
            1_000,
        )
        .unwrap();
        insert_document(
            &conn,
            Some(session_b),
            "job_description",
            "other.pdf",
            "other session",
            1_500,
        )
        .unwrap();

        let a_docs = list_for_session(&conn, session_a).unwrap();
        let names: Vec<&str> = a_docs.iter().map(|d| d.file_name.as_str()).collect();
        assert_eq!(names, vec!["first.pdf", "second.pdf"]);

        let b_docs = list_for_session(&conn, session_b).unwrap();
        assert_eq!(b_docs.len(), 1);
        assert_eq!(b_docs[0].file_name, "other.pdf");
    }

    #[test]
    fn list_recent_orders_newest_first_filters_by_doc_type_and_respects_the_limit() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        insert_document(&conn, None, "resume", "oldest.pdf", "r1", 1_000).unwrap();
        insert_document(&conn, None, "resume", "middle.pdf", "r2", 2_000).unwrap();
        insert_document(&conn, None, "resume", "newest.pdf", "r3", 3_000).unwrap();
        insert_document(
            &conn,
            None,
            "job_description",
            "jd.pdf",
            "not a resume",
            4_000,
        )
        .unwrap();

        let resumes = list_recent(&conn, "resume", 10).unwrap();
        let names: Vec<&str> = resumes.iter().map(|d| d.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["newest.pdf", "middle.pdf", "oldest.pdf"],
            "must be newest-first and exclude other doc types"
        );

        let limited = list_recent(&conn, "resume", 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].file_name, "newest.pdf");
    }

    #[test]
    fn attach_to_session_makes_the_picked_document_reachable_via_list_attached_for_session() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let doc_id = insert_document(
            &conn,
            None,
            "resume",
            "picked.pdf",
            "the one the owner picked",
            1_000,
        )
        .unwrap();
        attach_to_session(&conn, session_id, doc_id, "resume", 2_000).unwrap();

        let attached = list_attached_for_session(&conn, session_id).unwrap();
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].file_name, "picked.pdf");
    }

    #[test]
    fn the_wrong_resume_answered_bug_is_fixed_by_attaching_over_an_older_library_upload() {
        // Regression test for the reported bug: an older resume, uploaded
        // first into the session-less library, must not win over a newer
        // resume the owner explicitly picked and attached — even though
        // `list_for_context`'s own oldest-row-wins scan would still pick the
        // stale one if it were consulted instead of the attached set.
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "resume",
            "stale.pdf",
            "stale employer info",
            1_000,
        )
        .unwrap();
        let fresh_id = insert_document(
            &conn,
            None,
            "resume",
            "fresh.pdf",
            "correct employer info",
            5_000,
        )
        .unwrap();
        attach_to_session(&conn, session_id, fresh_id, "resume", 6_000).unwrap();

        let attached = list_attached_for_session(&conn, session_id).unwrap();
        assert_eq!(
            attached.len(),
            1,
            "only the explicitly attached document should be in the attached set"
        );
        assert_eq!(attached[0].raw_text, "correct employer info");

        // Sanity check: the stale scan really would have picked the wrong
        // one, proving the attached-set path is the thing doing the work.
        let scanned = context_from_rows(list_for_context(&conn, session_id).unwrap());
        assert_eq!(
            scanned.resume.as_deref(),
            Some("stale employer info"),
            "list_for_context's oldest-row-wins scan is expected to still pick the stale one on its own"
        );
    }

    #[test]
    fn re_attaching_a_different_document_of_the_same_type_makes_the_newest_pick_win() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let first_pick =
            insert_document(&conn, None, "resume", "v1.pdf", "version one", 1_000).unwrap();
        let second_pick =
            insert_document(&conn, None, "resume", "v2.pdf", "version two", 2_000).unwrap();
        attach_to_session(&conn, session_id, first_pick, "resume", 3_000).unwrap();
        attach_to_session(&conn, session_id, second_pick, "resume", 4_000).unwrap();

        let ctx = context_from_rows(list_attached_for_session(&conn, session_id).unwrap());
        assert_eq!(
            ctx.resume.as_deref(),
            Some("version two"),
            "the most recently attached pick must win"
        );
    }

    #[test]
    fn a_session_that_never_attached_anything_has_an_empty_attached_set() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        assert!(list_attached_for_session(&conn, session_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn attaching_other_documents_covers_the_same_bug_class_for_extra_context_files() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "other",
            "stale_notes.txt",
            "outdated notes",
            1_000,
        )
        .unwrap();
        let fresh = insert_document(
            &conn,
            None,
            "other",
            "fresh_notes.txt",
            "current notes",
            5_000,
        )
        .unwrap();
        attach_to_session(&conn, session_id, fresh, "other", 6_000).unwrap();

        let ctx = context_from_rows(list_attached_for_session(&conn, session_id).unwrap());
        assert_eq!(ctx.other_docs.len(), 1);
        assert_eq!(ctx.other_docs[0].1, "current notes");
    }

    /// REGRESSION TEST for the owner's 2026-08-28 report: "I am about to
    /// paste the job description from clipboard, I pressed Enter and the
    /// app suddenly shut." Exercises `upsert_document` with exactly the
    /// shape `commands::documents::ingest_text_document` builds from a real
    /// Documents-screen paste — **no active session** (the owner's own
    /// repro steps in `CURRENT.md` paste the job description *before*
    /// starting a session, so `session_id` really is `None` here, not a
    /// simplification), embedded newlines (what Enter actually inserts into
    /// a `<textarea>`, confirmed by reading `PasteTextCard.tsx`/`TextArea.tsx`:
    /// no `onKeyDown`, no `<form>` anywhere in `src/`, so Enter can only ever
    /// reach here as a `\n` byte inside `text`, never as a second, distinct
    /// save trigger), and the kind of unicode a real job posting copied off
    /// a web page carries (curly quotes, an em dash, a bullet, an emoji).
    /// Real root cause of the reported crash was proven (via the actual
    /// macOS crash reports in `~/Library/Logs/DiagnosticReports/Retired/`)
    /// to be a main-thread native-event-dispatch panic unrelated to this
    /// code path at all (see `panic_guard`'s doc comment) — this test
    /// exists to make that finding concrete and keep it true.
    #[test]
    fn pasting_a_realistic_job_description_with_no_session_and_an_enter_key_newline_does_not_panic()
    {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();

        let pasted_text = "Senior Backend Engineer — Acme Corp\n\
            \n\
            We're looking for someone who can:\n\
            • Own our payments pipeline end to end\n\
            • Mentor two junior engineers\n\
            • Say “yes” to a fully remote role 🎉\n\
            \n\
            Requirements: 5+ years Rust or Go, comfortable with on-call.\n";

        let id = upsert_document(
            &conn,
            None,
            "job_description",
            "Pasted job description",
            pasted_text.trim(),
            1_000,
        )
        .expect("a session-less paste must still save");

        let stored = get(&conn, id)
            .unwrap()
            .expect("the saved paste must be readable back");
        assert_eq!(stored.session_id, None);
        assert!(stored.raw_text.contains("Acme Corp"));
        assert!(
            stored.raw_text.contains('\n'),
            "the Enter-inserted newlines must survive, not be rejected"
        );

        // Editing the draft (pasting again after more typing, the owner's
        // actual next action) must replace it cleanly, still with no
        // session — this is the exact second call `PasteTextCard`'s Save
        // button makes if pressed twice.
        let edited = format!("{pasted_text}\nAdded after a second look.");
        let second_id = upsert_document(
            &conn,
            None,
            "job_description",
            "Pasted job description",
            edited.trim(),
            2_000,
        )
        .expect("re-saving a session-less paste must still work");
        assert_ne!(id, second_id);
        assert!(
            get(&conn, id).unwrap().is_none(),
            "the first session-less paste must be replaced, not duplicated"
        );
    }

    #[test]
    fn upsert_document_replaces_a_previous_paste_of_the_same_field_instead_of_appending() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let first_id = upsert_document(
            &conn,
            Some(session_id),
            "job_description",
            "Pasted job description",
            "We need a backend engineer.",
            1_000,
        )
        .unwrap();
        let second_id = upsert_document(
            &conn,
            Some(session_id),
            "job_description",
            "Pasted job description",
            "We need a senior backend engineer with Rust experience.",
            2_000,
        )
        .unwrap();

        assert_ne!(
            first_id, second_id,
            "a fresh row should be inserted, not the old one mutated in place"
        );
        assert!(
            get(&conn, first_id).unwrap().is_none(),
            "the previous paste must be gone, not left behind"
        );

        let docs = list_for_session(&conn, session_id).unwrap();
        assert_eq!(
            docs.len(),
            1,
            "editing a paste must replace it, not grow the document list"
        );
        assert!(docs[0].raw_text.contains("senior backend engineer"));
    }

    #[test]
    fn upsert_document_leaves_documents_with_a_different_type_or_name_untouched() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            Some(session_id),
            "resume",
            "resume.pdf",
            "an uploaded resume",
            500,
        )
        .unwrap();
        upsert_document(
            &conn,
            Some(session_id),
            "job_description",
            "Pasted job description",
            "posting text",
            1_000,
        )
        .unwrap();
        upsert_document(
            &conn,
            Some(session_id),
            "other",
            "Pasted extra context",
            "extra notes",
            1_500,
        )
        .unwrap();

        let docs = list_for_session(&conn, session_id).unwrap();
        assert_eq!(
            docs.len(),
            3,
            "documents of a different type or name must not be touched by an unrelated upsert"
        );
    }

    #[test]
    fn delete_removes_the_document_and_cascades_to_its_chunks() {
        use crate::storage::chunks;

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let doc_id = insert_document(
            &conn,
            Some(session_id),
            "resume",
            "resume.pdf",
            "resume text",
            1_000,
        )
        .unwrap();
        chunks::insert_chunk(
            &conn,
            session_id,
            Some(doc_id),
            "document",
            "resume text",
            None,
            None,
            1_000,
        )
        .unwrap();
        assert_eq!(chunks::count_for_session(&conn, session_id).unwrap(), 1);

        let deleted = delete(&conn, doc_id).unwrap();
        assert!(deleted);
        assert!(get(&conn, doc_id).unwrap().is_none());
        assert_eq!(
            chunks::count_for_session(&conn, session_id).unwrap(),
            0,
            "the schema's ON DELETE CASCADE on chunks.document_id should remove the document's chunks too"
        );
    }

    #[test]
    fn delete_of_a_missing_document_returns_false_without_erroring() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(!delete(&conn, Uuid::new_v4()).unwrap());
    }

    #[test]
    fn a_pasted_document_loaded_back_and_mapped_into_default_context_reaches_render() {
        // Proves the upserted text is not stranded in storage: mapped into
        // `window_memory::DefaultContext` the same way
        // `window_memory::load_default_context_from_store` maps a session's
        // documents, it shows up in the exact `render()` a real prompt uses.
        // Calling `load_default_context_from_store` itself would need a real
        // `tauri::AppHandle`, which this codebase has no test fixture for
        // yet (same gap `active_session.rs`'s own tests already note).
        use crate::window_memory::DefaultContext;

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        upsert_document(
            &conn,
            Some(session_id),
            "job_description",
            "Pasted job description",
            "We need someone who knows distributed systems.",
            1_000,
        )
        .unwrap();

        let mut ctx = DefaultContext::default();
        for doc in list_for_session(&conn, session_id).unwrap() {
            match doc.doc_type.as_str() {
                "resume" if ctx.resume.is_none() => ctx.resume = Some(doc.raw_text),
                "job_description" if ctx.job_description.is_none() => {
                    ctx.job_description = Some(doc.raw_text)
                }
                _ => ctx.other_docs.push((doc.file_name, doc.raw_text)),
            }
        }

        assert!(ctx.render().contains("distributed systems"));
    }

    /// Maps `list_for_context`'s rows into a `DefaultContext` exactly the
    /// way `window_memory::load_default_context_from_store` does — kept
    /// local (rather than calling that private function) for the same
    /// no-`AppHandle`-fixture reason `a_pasted_document_loaded_back...`
    /// above already explains.
    fn context_from_rows(docs: Vec<StoredDocument>) -> crate::window_memory::DefaultContext {
        let mut ctx = crate::window_memory::DefaultContext::default();
        for doc in docs {
            match doc.doc_type.as_str() {
                "resume" if ctx.resume.is_none() => ctx.resume = Some(doc.raw_text),
                "job_description" if ctx.job_description.is_none() => {
                    ctx.job_description = Some(doc.raw_text)
                }
                _ => ctx.other_docs.push((doc.file_name, doc.raw_text)),
            }
        }
        ctx
    }

    #[test]
    fn a_session_less_document_reaches_default_context_for_a_newly_created_session() {
        // The bug this fix closes: a resume uploaded with no active session
        // (`session_id = NULL`) must still be seen by a session created
        // afterward, not silently dropped forever.
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "resume",
            "resume.pdf",
            "Jordan Rivera, 5 years of Rust.",
            500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        assert_eq!(
            ctx.resume.as_deref(),
            Some("Jordan Rivera, 5 years of Rust.")
        );
    }

    #[test]
    fn a_session_less_document_also_reaches_a_second_later_session_proving_it_is_a_library() {
        // Proves the session-less document is a reusable library entry, not
        // consumed by the first session that reads it.
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "resume",
            "resume.pdf",
            "Jordan Rivera, 5 years of Rust.",
            500,
        )
        .unwrap();

        let ctx_a = context_from_rows(list_for_context(&conn, session_a).unwrap());
        let ctx_b = context_from_rows(list_for_context(&conn, session_b).unwrap());
        assert_eq!(
            ctx_a.resume.as_deref(),
            Some("Jordan Rivera, 5 years of Rust.")
        );
        assert_eq!(
            ctx_b.resume.as_deref(),
            Some("Jordan Rivera, 5 years of Rust.")
        );
    }

    #[test]
    fn a_real_uploaded_pdf_resume_reaches_the_exact_prompt_string_sent_to_the_llm() {
        // Closes the one link in the document pipeline that no existing test
        // chained together: real PDF bytes -> `ingest::extract_text` ->
        // stored `raw_text` -> `DefaultContext` -> the literal `system`
        // string a real answer request sends. Every other link already had
        // its own test (extraction alone, against this same fixture, in
        // `ingest::mod`'s tests; storage-to-prompt, with hand-typed text
        // standing in for a document, just above and in `window_memory::prompt`'s
        // own tests) — this proves the whole chain together, with a real
        // file, not a synthetic string pretending to be one.
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ingest/sample_resume.pdf");
        let extracted =
            crate::ingest::extract_text(&fixture).expect("real fixture must extract cleanly");
        assert!(
            extracted.contains("Jordan Rivera"),
            "sanity check on the extractor itself, before testing anything downstream of it"
        );

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_document(
            &conn,
            Some(session_id),
            "resume",
            "sample_resume.pdf",
            &extracted,
            500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        let window = crate::window_memory::ConversationWindow {
            turns: vec![],
            start_ms: 0,
            end_ms: 0,
        };
        let prompt = crate::window_memory::build_prompt(
            &ctx,
            &crate::window_memory::SessionMemory::default(),
            &window,
        );

        assert!(
            prompt.system.contains("Jordan Rivera"),
            "the real extracted PDF text must appear verbatim in the exact system string a real answer request sends, not just somewhere in the pipeline"
        );
    }

    #[test]
    fn a_real_uploaded_docx_resume_reaches_the_exact_prompt_string_sent_to_the_llm() {
        // Same proof as the PDF test above, for the other supported format —
        // DOCX goes through a structurally different extractor (`docx-rs`
        // walking the OOXML paragraph/table tree, not `pdf-extract`), so it
        // is not covered by the PDF test.
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ingest/sample_resume.docx");
        let extracted =
            crate::ingest::extract_text(&fixture).expect("real fixture must extract cleanly");
        assert!(
            extracted.contains("Jordan Rivera"),
            "sanity check on the extractor itself, before testing anything downstream of it"
        );

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_document(
            &conn,
            Some(session_id),
            "resume",
            "sample_resume.docx",
            &extracted,
            500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        let window = crate::window_memory::ConversationWindow {
            turns: vec![],
            start_ms: 0,
            end_ms: 0,
        };
        let prompt = crate::window_memory::build_prompt(
            &ctx,
            &crate::window_memory::SessionMemory::default(),
            &window,
        );

        assert!(
            prompt.system.contains("Jordan Rivera"),
            "the real extracted DOCX text must appear verbatim in the exact system string a real answer request sends"
        );
    }

    #[test]
    fn a_pasted_job_description_reaches_the_exact_prompt_string_sent_to_the_llm() {
        // The text-field path (`commands::documents::ingest_text_document`,
        // which calls `upsert_document` directly — there is no file to
        // parse, so `ingest::extract_text` is never involved) needs its own
        // proof: pasting a job description, not uploading a file, must reach
        // the same real prompt string just as reliably.
        let pasted =
            "We are hiring a Staff Data Engineer.\nMust have deep Spark and Kafka experience.";
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        upsert_document(
            &conn,
            Some(session_id),
            "job_description",
            "Pasted job description",
            pasted,
            500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        let window = crate::window_memory::ConversationWindow {
            turns: vec![],
            start_ms: 0,
            end_ms: 0,
        };
        let prompt = crate::window_memory::build_prompt(
            &ctx,
            &crate::window_memory::SessionMemory::default(),
            &window,
        );

        assert!(
            prompt.system.contains("Staff Data Engineer") && prompt.system.contains("Spark and Kafka"),
            "pasted job-description text must reach the exact system string a real answer request sends, not just the database"
        );
    }

    #[test]
    fn a_session_specific_resume_overrides_a_session_less_resume() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "resume",
            "old_resume.pdf",
            "Library resume, out of date.",
            500,
        )
        .unwrap();
        insert_document(
            &conn,
            Some(session_id),
            "resume",
            "new_resume.pdf",
            "This session's resume.",
            1_500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        assert_eq!(
            ctx.resume.as_deref(),
            Some("This session's resume."),
            "the session-specific resume must win over the library one, regardless of upload order"
        );
    }

    #[test]
    fn session_less_and_session_specific_other_docs_both_appear() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "other",
            "standing_notes.pdf",
            "Always mention I am open to relocation.",
            500,
        )
        .unwrap();
        insert_document(
            &conn,
            Some(session_id),
            "other",
            "interview_notes.pdf",
            "This company values ownership.",
            1_500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        let names: Vec<&str> = ctx
            .other_docs
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["interview_notes.pdf", "standing_notes.pdf"],
            "list_for_context orders this session's own documents before the session-less library"
        );
    }

    #[test]
    fn context_preview_char_count_matches_a_real_prompt_when_a_session_less_document_is_present() {
        // `get_context_preview` (`commands::session::get_context_preview`)
        // and a real answer both go through
        // `window_memory::load_default_context_from_store`, which this test
        // mirrors via `context_from_rows`/`list_for_context`. If the preview
        // used a different, stricter fetch than the real prompt, its
        // character count would under-report whenever a session-less
        // document exists.
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        insert_document(
            &conn,
            None,
            "resume",
            "resume.pdf",
            "Jordan Rivera, 5 years of Rust.",
            500,
        )
        .unwrap();

        let ctx = context_from_rows(list_for_context(&conn, session_id).unwrap());
        let preview = ctx.preview();
        let real_system = format!("{}\n\n{}", crate::window_memory::STRICT_RULES, ctx.render());
        assert_eq!(preview.system_char_count, real_system.len());
        assert!(
            preview.system_char_count > 0,
            "the session-less resume must actually be counted, not skipped"
        );
    }

    #[test]
    fn a_document_survives_closing_and_reopening_the_database_file() {
        // Mirrors `storage/transcripts.rs`'s equivalent real-restart proof.
        let path = std::env::temp_dir().join(format!("aside-doc-test-{}.sqlite3", Uuid::new_v4()));

        let (session_id, doc_id) = {
            let db = Db::open(&path).expect("first open");
            let conn = db.0.lock().unwrap();
            let session = Session::new("Document persistence test", 1_000);
            sessions::create(&conn, &session).unwrap();
            let doc_id = insert_document(
                &conn,
                Some(session.id),
                "resume",
                "survives.pdf",
                "this should survive",
                2_000,
            )
            .unwrap();
            (session.id, doc_id)
        };

        let reloaded = {
            let db = Db::open(&path).expect("second open");
            let conn = db.0.lock().unwrap();
            get(&conn, doc_id).unwrap()
        };

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));

        let reloaded = reloaded.expect("document should have persisted");
        assert_eq!(reloaded.raw_text, "this should survive");
        assert_eq!(reloaded.session_id, Some(session_id));
    }

    /// REGRESSION TEST for the owner's own stated requirement: "Additional
    /// Documents are multi-selectable, Resume is one at a time — all the
    /// documents selected in the session window are to be sent to LLM."
    /// Exercises the exact real path (`attach_to_session`, one call per
    /// selected doc — what `useCreateForm.ts`'s submit loop does for every
    /// item in `[selectedResume, ...selectedDocs]`) with **one** resume and
    /// **three** additional documents attached to the same session, then
    /// proves every one of them — not just the first or the last — appears
    /// in the exact `system` string a real answer request sends.
    #[test]
    fn every_multi_selected_additional_document_reaches_the_exact_prompt_string_sent_to_the_llm() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let resume_id = insert_document(
            &conn,
            None,
            "resume",
            "resume.pdf",
            "Jordan Rivera, 5 years of Rust.",
            100,
        )
        .unwrap();
        let notes_id = insert_document(
            &conn,
            None,
            "other",
            "prep_notes.pdf",
            "Remember to mention the Kafka migration.",
            200,
        )
        .unwrap();
        let company_id = insert_document(
            &conn,
            None,
            "other",
            "company_research.pdf",
            "Acme just raised a Series C led by Northwind.",
            300,
        )
        .unwrap();
        let values_id = insert_document(
            &conn,
            None,
            "other",
            "values.pdf",
            "Acme's stated value: bias toward ownership.",
            400,
        )
        .unwrap();

        // Mirrors `useCreateForm.ts`'s `submit()`: one `attach_document_to_session`
        // call per item in `[selectedResume, ...selectedDocs]`, in selection order.
        for (id, doc_type) in [
            (resume_id, "resume"),
            (notes_id, "other"),
            (company_id, "other"),
            (values_id, "other"),
        ] {
            attach_to_session(&conn, session_id, id, doc_type, 1_000).unwrap();
        }

        let attached = list_attached_for_session(&conn, session_id).unwrap();
        assert_eq!(
            attached.len(),
            4,
            "all four explicitly-attached documents must come back, not just one"
        );

        let ctx = context_from_rows(attached);
        assert_eq!(
            ctx.resume.as_deref(),
            Some("Jordan Rivera, 5 years of Rust.")
        );
        assert_eq!(
            ctx.other_docs.len(),
            3,
            "every selected additional document must reach DefaultContext, not just the first"
        );

        let window = crate::window_memory::ConversationWindow {
            turns: vec![],
            start_ms: 0,
            end_ms: 0,
        };
        let prompt = crate::window_memory::build_prompt(
            &ctx,
            &crate::window_memory::SessionMemory::default(),
            &window,
        );

        assert!(
            prompt.system.contains("Jordan Rivera, 5 years of Rust."),
            "the resume must reach the real prompt"
        );
        assert!(
            prompt.system.contains("Kafka migration"),
            "the first additional document must reach the real prompt: {}",
            prompt.system
        );
        assert!(
            prompt.system.contains("Series C led by Northwind"),
            "the second additional document must reach the real prompt: {}",
            prompt.system
        );
        assert!(
            prompt.system.contains("bias toward ownership"),
            "the third additional document must reach the real prompt — a system that only sent the first \
             or last selected document would fail exactly this assertion: {}",
            prompt.system
        );
    }
}
