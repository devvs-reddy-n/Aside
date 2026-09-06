//! Cross-table reads for the Past Sessions feature (a permanent, browsable
//! record of what was actually said in each session).
//!
//! Neither query here belongs cleanly in `sessions.rs` (session-only
//! columns) or `transcripts.rs` (single-session-scoped reads): the session
//! list needs a per-session segment count, and search needs a session's
//! title alongside its matching lines — both genuinely join `sessions` and
//! `transcript_segments`, so they live together here rather than being
//! forced into either single-table module. Same conventions as the rest of
//! `storage/`: plain functions over a `&Connection`, row-mapping helpers,
//! `String`-formatted errors.
//!
//! **Search is a direct `LIKE` query on `transcript_segments`, not FTS5.**
//! The existing `chunks_fts` index (`storage/chunks.rs::search_fts`) covers
//! `rag::indexer`'s ~80-word *chunks* — merged windows of transcript text
//! that deliberately drop per-utterance speaker and timestamp to make good
//! embedding/retrieval units. That's the wrong shape for a transcript
//! search result, which needs to say who said which exact line and when.
//! Rather than build a second FTS5 index (a schema/migration change) just
//! for this, a plain `LIKE '%...%'` over `transcript_segments` — a table
//! already indexed on `(session_id, start_ms)` — is enough for a personal,
//! single-user transcript archive.

use rusqlite::{params, Connection};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::SessionStatus;

use super::sessions::status_from_str;

/// One row of the Past Sessions list: enough to show "what was this, when,
/// how much was actually said" without a second round trip per session.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PastSessionSummary {
    pub id: Uuid,
    pub title: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    /// `None` for a session that never ended (or is still active) — there is
    /// no honest duration to report yet.
    pub duration_ms: Option<u64>,
    pub status: SessionStatus,
    pub company: Option<String>,
    /// Count of `is_final = 1` transcript segments only — a session with
    /// nothing but interim/non-final rows (or none at all) should read as
    /// empty, not as having real recorded speech.
    pub segment_count: i64,
}

fn row_to_summary(row: &rusqlite::Row) -> rusqlite::Result<PastSessionSummary> {
    let id: String = row.get("id")?;
    let status_str: String = row.get("status")?;
    let started_at_ms: u64 = row.get("started_at_ms")?;
    let ended_at_ms: Option<u64> = row.get("ended_at_ms")?;
    Ok(PastSessionSummary {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        title: row.get("title")?,
        started_at_ms,
        ended_at_ms,
        duration_ms: ended_at_ms.map(|end| end.saturating_sub(started_at_ms)),
        status: status_from_str(&status_str).unwrap_or(SessionStatus::Draft),
        company: row.get("company")?,
        segment_count: row.get("segment_count")?,
    })
}

/// Every session, newest first, each carrying its final-segment count — one
/// query with a `LEFT JOIN` + `GROUP BY`, not N+1 (a `COUNT` per session in
/// a loop) — so an empty session (0 final segments, e.g. dropped
/// immediately after creation) still shows up with `segment_count: 0`
/// rather than being silently excluded by the join.
pub fn list_with_segment_counts(conn: &Connection) -> Result<Vec<PastSessionSummary>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.started_at_ms, s.ended_at_ms, s.status, s.company,
                    COUNT(t.id) AS segment_count
             FROM sessions s
             LEFT JOIN transcript_segments t
               ON t.session_id = s.id AND t.is_final = 1
             GROUP BY s.id
             ORDER BY s.started_at_ms DESC",
        )
        .map_err(|e| format!("failed to prepare past-sessions list query: {e}"))?;
    let rows = stmt
        .query_map([], row_to_summary)
        .map_err(|e| format!("failed to run past-sessions list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read past-sessions row: {e}"))
}

/// One search hit: which session, its title (so the frontend never needs a
/// second lookup to render a result), which line, who said it, and when.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSearchHit {
    pub session_id: Uuid,
    pub session_title: String,
    pub channel: String,
    pub text: String,
    pub start_ms: u64,
}

fn row_to_hit(row: &rusqlite::Row) -> rusqlite::Result<TranscriptSearchHit> {
    let session_id: String = row.get("session_id")?;
    Ok(TranscriptSearchHit {
        session_id: Uuid::parse_str(&session_id).unwrap_or_else(|_| Uuid::nil()),
        session_title: row.get("title")?,
        channel: row.get("channel")?,
        text: row.get("text")?,
        start_ms: row.get("start_ms")?,
    })
}

/// Escapes `LIKE`'s three special characters so a search phrase containing
/// `%` or `_` is matched literally rather than as a wildcard.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Searches `is_final = 1` transcript text for `query`, optionally scoped to
/// one session. `session_id = None` searches every session. An empty/blank
/// query returns no results rather than every row in the database.
pub fn search(
    conn: &Connection,
    query: &str,
    session_id: Option<Uuid>,
) -> Result<Vec<TranscriptSearchHit>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let like_pattern = format!("%{}%", escape_like(trimmed));

    let rows = match session_id {
        Some(id) => {
            let mut stmt = conn
                .prepare(
                    "SELECT t.session_id, s.title, t.channel, t.text, t.start_ms
                     FROM transcript_segments t
                     JOIN sessions s ON s.id = t.session_id
                     WHERE t.is_final = 1 AND t.text LIKE ?1 ESCAPE '\\' AND t.session_id = ?2
                     ORDER BY t.start_ms ASC",
                )
                .map_err(|e| format!("failed to prepare scoped transcript search query: {e}"))?;
            let mapped = stmt
                .query_map(params![like_pattern, id.to_string()], row_to_hit)
                .map_err(|e| format!("failed to run scoped transcript search query: {e}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read scoped transcript search row: {e}"))?;
            mapped
        }
        None => {
            let mut stmt = conn
                .prepare(
                    "SELECT t.session_id, s.title, t.channel, t.text, t.start_ms
                     FROM transcript_segments t
                     JOIN sessions s ON s.id = t.session_id
                     WHERE t.is_final = 1 AND t.text LIKE ?1 ESCAPE '\\'
                     ORDER BY t.start_ms ASC",
                )
                .map_err(|e| format!("failed to prepare transcript search query: {e}"))?;
            let mapped = stmt
                .query_map(params![like_pattern], row_to_hit)
                .map_err(|e| format!("failed to run transcript search query: {e}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read transcript search row: {e}"))?;
            mapped
        }
    };
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{sessions, transcripts, Db};

    fn seeded_session(db: &Db, title: &str, started_at_ms: u64) -> Uuid {
        let conn = db.0.lock().unwrap();
        let session = Session::new(title, started_at_ms);
        sessions::create(&conn, &session).unwrap();
        session.id
    }

    #[test]
    fn list_with_segment_counts_orders_newest_first_and_counts_only_finals() {
        let db = Db::open_in_memory();
        let older = seeded_session(&db, "older", 1_000);
        let newer = seeded_session(&db, "newer", 2_000);
        let conn = db.0.lock().unwrap();
        transcripts::insert_segment(
            &conn,
            older,
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
        transcripts::insert_segment(
            &conn,
            older,
            "system",
            "local_parakeet",
            "hey",
            100,
            200,
            None,
            true,
            200,
        )
        .unwrap();
        transcripts::insert_segment(
            &conn,
            older,
            "mic",
            "local_parakeet",
            "typing...",
            200,
            210,
            None,
            false,
            210,
        )
        .unwrap();
        // `newer` has zero segments — must still show up, with count 0.

        let summaries = list_with_segment_counts(&conn).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, newer);
        assert_eq!(summaries[0].segment_count, 0);
        assert_eq!(summaries[1].id, older);
        assert_eq!(
            summaries[1].segment_count, 2,
            "non-final row must not be counted"
        );
    }

    #[test]
    fn list_with_segment_counts_reports_duration_only_for_ended_sessions() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut ended = Session::new("ended", 1_000);
        ended.ended_at_ms = Some(5_500);
        sessions::create(&conn, &ended).unwrap();
        let still_open = Session::new("still open", 2_000);
        sessions::create(&conn, &still_open).unwrap();

        let summaries = list_with_segment_counts(&conn).unwrap();
        let ended_summary = summaries.iter().find(|s| s.id == ended.id).unwrap();
        let open_summary = summaries.iter().find(|s| s.id == still_open.id).unwrap();
        assert_eq!(ended_summary.duration_ms, Some(4_500));
        assert_eq!(open_summary.duration_ms, None);
    }

    #[test]
    fn search_finds_a_phrase_across_every_session() {
        let db = Db::open_in_memory();
        let a = seeded_session(&db, "Session A", 1_000);
        let b = seeded_session(&db, "Session B", 2_000);
        let conn = db.0.lock().unwrap();
        transcripts::insert_segment(
            &conn,
            a,
            "mic",
            "local_parakeet",
            "I'm authorized on STEM OPT",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        transcripts::insert_segment(
            &conn,
            b,
            "system",
            "local_parakeet",
            "Tell me about your Kubernetes experience",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();

        let hits = search(&conn, "STEM OPT", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, a);
        assert_eq!(hits[0].session_title, "Session A");
        assert_eq!(hits[0].channel, "mic");
    }

    #[test]
    fn search_scoped_to_a_session_only_returns_that_sessions_hits() {
        let db = Db::open_in_memory();
        let a = seeded_session(&db, "Session A", 1_000);
        let b = seeded_session(&db, "Session B", 2_000);
        let conn = db.0.lock().unwrap();
        transcripts::insert_segment(
            &conn,
            a,
            "mic",
            "local_parakeet",
            "we use Kubernetes daily",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();
        transcripts::insert_segment(
            &conn,
            b,
            "mic",
            "local_parakeet",
            "we also use Kubernetes here",
            0,
            100,
            None,
            true,
            100,
        )
        .unwrap();

        let hits = search(&conn, "Kubernetes", Some(a)).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, a);
    }

    #[test]
    fn search_excludes_non_final_rows_and_a_blank_query_returns_nothing() {
        let db = Db::open_in_memory();
        let a = seeded_session(&db, "Session A", 1_000);
        let conn = db.0.lock().unwrap();
        transcripts::insert_segment(
            &conn,
            a,
            "mic",
            "local_parakeet",
            "still speaking about Rust",
            0,
            100,
            None,
            false,
            100,
        )
        .unwrap();

        assert!(search(&conn, "Rust", None).unwrap().is_empty());
        assert!(search(&conn, "   ", None).unwrap().is_empty());
    }
}
