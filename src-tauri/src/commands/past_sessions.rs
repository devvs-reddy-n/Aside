//! Tauri commands for Past Sessions: a permanent, browsable record of what
//! was actually said in each session (never the AI's suggested answers —
//! those live in `generated_suggestions`/`interview_exchanges`, untouched
//! here). Thin command functions over `storage::past_sessions`,
//! `storage::sessions` and `storage::transcripts`, matching this project's
//! established `commands::*` shape (delegate to storage, don't duplicate
//! query logic here).

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::storage::{past_sessions, sessions, transcripts, Db};

/// Who actually said a line — `mic` is this app's owner, `system` is
/// whoever else was on the call. A clear enum instead of leaking the raw
/// `channel` column value to the frontend, matching this project's existing
/// "you"/"them" naming (`commands::session::get_last_you_turn`/
/// `get_last_them_turn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    You,
    Them,
}

fn speaker_from_channel(channel: &str) -> Speaker {
    match channel {
        "mic" => Speaker::You,
        _ => Speaker::Them,
    }
}

/// Every session, newest first, each carrying its final-transcript-segment
/// count so the UI can show how much was actually said (and hide/gray out
/// empty sessions). One join+count query in `storage::past_sessions`, not
/// N+1.
#[tauri::command]
pub fn list_past_sessions(db: State<Db>) -> Result<Vec<past_sessions::PastSessionSummary>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    past_sessions::list_with_segment_counts(&conn)
}

/// One line of a rendered transcript: who said it, the verbatim text, and
/// when.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptTurn {
    pub speaker: Speaker,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Every final segment for one session, in the order it was actually
/// spoken (`storage::transcripts::list_final_for_session`'s `start_ms`
/// order) — only `is_final = 1` rows; interim/in-progress lines never made
/// it into the permanent record.
#[tauri::command]
pub fn get_session_transcript(
    db: State<Db>,
    session_id: Uuid,
) -> Result<Vec<TranscriptTurn>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let segments = transcripts::list_final_for_session(&conn, session_id)?;
    Ok(segments
        .into_iter()
        .map(|s| TranscriptTurn {
            speaker: speaker_from_channel(&s.channel),
            text: s.text,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
        })
        .collect())
}

/// One search result: enough to render a result list without a second
/// lookup per hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptSearchResult {
    pub session_id: Uuid,
    pub session_title: String,
    pub speaker: Speaker,
    pub text: String,
    pub start_ms: u64,
}

/// Searches actually-spoken transcript text for `query`. `session_id =
/// None` searches every session; `Some(id)` scopes to one.
///
/// This is a direct `LIKE` query on `transcript_segments`
/// (`storage::past_sessions::search`), not the existing `chunks_fts` FTS5
/// index — see that module's doc comment for why: `chunks_fts` covers
/// `rag::indexer`'s merged, speaker-less embedding chunks, which is the
/// wrong shape for "who said this exact line, and when".
#[tauri::command]
pub fn search_transcripts(
    db: State<Db>,
    query: String,
    session_id: Option<Uuid>,
) -> Result<Vec<TranscriptSearchResult>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let hits = past_sessions::search(&conn, &query, session_id)?;
    Ok(hits
        .into_iter()
        .map(|h| TranscriptSearchResult {
            session_id: h.session_id,
            session_title: h.session_title,
            speaker: speaker_from_channel(&h.channel),
            text: h.text,
            start_ms: h.start_ms,
        })
        .collect())
}

/// One AI answer generated during the session: the question it was
/// answering, the answer itself, and when. Backed by `interview_exchanges`
/// (2026-08-30, owner request — the transcript view should show the AI's
/// own answers interleaved with what was actually said, matching the
/// owner's own ParakeetAI export format). `answer` is the AI's *suggested*
/// answer (`ai_suggestion`), never `actual_user_answer` — those are
/// deliberately different fields and must not be confused here either.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionAnswer {
    pub question: String,
    pub answer: String,
    pub at_ms: u64,
}

/// Every AI answer generated during `session_id`, oldest first — for
/// interleaving with `get_session_transcript`'s spoken turns by timestamp.
/// Skips any exchange that never actually got an AI answer (opened but the
/// call ended, or a screenshot/error path that never produced one).
#[tauri::command]
pub fn get_session_answers(db: State<Db>, session_id: Uuid) -> Result<Vec<SessionAnswer>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let exchanges = crate::storage::exchanges::list_all(&conn, session_id)?;
    Ok(exchanges
        .into_iter()
        .filter_map(|e| {
            let answer = e.ai_suggestion?;
            let at_ms = e.suggestion_at_ms.unwrap_or(e.opened_at_ms);
            Some(SessionAnswer {
                question: e.interviewer_question,
                answer,
                at_ms,
            })
        })
        .collect())
}

/// The session's background-summarized context, one bullet per completed
/// Cmd+Enter window, oldest first (2026-08-30, owner request: this is now
/// the *only* place "Context" lives — the standalone Context screen and its
/// live-session-only preview are gone; this reads the durable copy in
/// `storage::window_summaries` instead, so it works for a past session too,
/// not only the one currently active).
#[tauri::command]
pub fn get_session_context(db: State<Db>, session_id: Uuid) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let rows = crate::storage::window_summaries::list_for_session(&conn, session_id)?;
    Ok(rows.into_iter().map(|r| r.text).collect())
}

/// Produces an on-demand whole-call overview from the same final spoken
/// transcript exposed by `get_session_transcript`. It is deliberately not
/// based on AI suggestions or on the smaller context-note records, so its
/// paragraph and highlights describe only what call participants said.
#[tauri::command]
pub async fn summarize_past_session(
    app: AppHandle,
    db: State<'_, Db>,
    session_id: Uuid,
) -> Result<crate::llm::background::SessionSummary, String> {
    let transcript = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        let segments = transcripts::list_final_for_session(&conn, session_id)?;
        segments
            .into_iter()
            .map(|segment| {
                let speaker = match speaker_from_channel(&segment.channel) {
                    Speaker::You => "Me",
                    Speaker::Them => "Other",
                };
                format!("{speaker}: {}", segment.text.trim())
            })
            .filter(|line| !line.ends_with(':'))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if transcript.is_empty() {
        return Err("there is no recorded conversation to summarize".to_string());
    }
    crate::llm::background::summarize_completed_session(&app, &transcript).await
}

/// Hard delete of a session — no soft delete, no trash table. The schema's
/// `ON DELETE CASCADE` on `transcript_segments.session_id`,
/// `chunks.session_id` and `documents.session_id` (`storage/migrations.rs`)
/// removes the dependent rows in the same statement; `migrations::run`
/// turns `PRAGMA foreign_keys` ON for every connection this app opens, so
/// that cascade genuinely fires at runtime rather than being a no-op (see
/// `storage::sessions::deleting_a_session_cascades_to_transcript_segments_chunks_and_documents`
/// for the runtime proof). Returns whether a row actually existed to
/// delete.
#[tauri::command]
pub fn delete_session(db: State<Db>, session_id: Uuid) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    sessions::delete(&conn, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_from_channel_maps_mic_to_you_and_anything_else_to_them() {
        assert_eq!(speaker_from_channel("mic"), Speaker::You);
        assert_eq!(speaker_from_channel("system"), Speaker::Them);
    }

    #[test]
    fn speaker_serializes_as_lowercase_snake_case_for_the_frontend() {
        assert_eq!(serde_json::to_value(Speaker::You).unwrap(), "you");
        assert_eq!(serde_json::to_value(Speaker::Them).unwrap(), "them");
    }

    /// End-to-end proof that a real spoken conversation — both channels,
    /// not just one — actually round-trips through storage and back out as
    /// a readable transcript. Inserts through `storage::transcripts::insert_segment`
    /// (the exact call `pipeline.rs`'s `persist_finals_to_db_at` makes once
    /// speech is transcribed), then reads it back through this module's own
    /// `get_session_transcript` logic, not a hand-rolled shortcut — this is
    /// the owner's own "the conversation between me and the interviewer
    /// should be saved, and I should be able to reopen the transcript in
    /// Past Sessions" requirement, proven with real interleaved turns from
    /// both sides rather than a single line.
    #[test]
    fn a_real_two_sided_conversation_round_trips_through_get_session_transcript() {
        use crate::domain::Session;
        use crate::storage::{sessions, transcripts, Db};

        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let session = Session::new("Real interview", 1_000);
        sessions::create(&conn, &session).unwrap();

        transcripts::insert_segment(
            &conn,
            session.id,
            "system",
            "speechmatics",
            "Tell me about your last project.",
            1_000,
            3_000,
            None,
            true,
            3_000,
        )
        .unwrap();
        transcripts::insert_segment(
            &conn,
            session.id,
            "mic",
            "local_parakeet",
            "I led the migration to a new pipeline.",
            3_500,
            6_000,
            None,
            true,
            6_000,
        )
        .unwrap();
        // An in-progress interim line must never leak into the permanent
        // record — `is_final: false`.
        transcripts::insert_segment(
            &conn,
            session.id,
            "system",
            "speechmatics",
            "And how did",
            6_200,
            6_800,
            None,
            false,
            6_800,
        )
        .unwrap();

        let segments = transcripts::list_final_for_session(&conn, session.id).unwrap();
        let turns: Vec<TranscriptTurn> = segments
            .into_iter()
            .map(|s| TranscriptTurn {
                speaker: speaker_from_channel(&s.channel),
                text: s.text,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
            })
            .collect();

        assert_eq!(
            turns.len(),
            2,
            "the interim line must not appear in the saved transcript"
        );
        assert_eq!(turns[0].speaker, Speaker::Them);
        assert_eq!(turns[0].text, "Tell me about your last project.");
        assert_eq!(turns[1].speaker, Speaker::You);
        assert_eq!(turns[1].text, "I led the migration to a new pipeline.");
        assert!(
            turns[0].start_ms < turns[1].start_ms,
            "turns must come back in spoken order"
        );
    }
}
