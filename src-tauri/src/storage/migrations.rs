use rusqlite::Connection;

/// The full schema, as of migration 1. Later migrations should be added as
/// additional `if version < N` blocks below `apply_v1`, never by editing this
/// one — `PRAGMA user_version` records how far a given database has been
/// brought forward, so this must stay a append-only history.
const V1_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id            TEXT PRIMARY KEY,
  title         TEXT NOT NULL,
  mode_id       TEXT REFERENCES modes(id),
  participants  TEXT NOT NULL DEFAULT '[]',
  started_at_ms INTEGER NOT NULL,
  ended_at_ms   INTEGER,
  status        TEXT NOT NULL CHECK (status IN ('draft','active','ended')),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS transcript_segments (
  id               TEXT PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  channel          TEXT NOT NULL CHECK (channel IN ('mic','system')),
  origin           TEXT NOT NULL CHECK (origin IN ('local_parakeet','deepgram','speechmatics')),
  text             TEXT NOT NULL,
  start_ms         INTEGER NOT NULL,
  end_ms           INTEGER NOT NULL,
  confidence       REAL,
  is_final         INTEGER NOT NULL DEFAULT 1,
  created_at_ms    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_transcript_segments_session ON transcript_segments(session_id, start_ms);

CREATE TABLE IF NOT EXISTS facts (
  id                TEXT PRIMARY KEY,
  session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  key               TEXT NOT NULL,
  value             TEXT NOT NULL,
  source_segment_id TEXT REFERENCES transcript_segments(id),
  confidence        REAL,
  updated_at_ms     INTEGER NOT NULL,
  UNIQUE (session_id, key)
);

CREATE TABLE IF NOT EXISTS documents (
  id            TEXT PRIMARY KEY,
  session_id    TEXT REFERENCES sessions(id) ON DELETE CASCADE,
  doc_type      TEXT NOT NULL CHECK (doc_type IN ('resume','job_description','other')),
  file_name     TEXT NOT NULL,
  raw_text      TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
  id            TEXT PRIMARY KEY,
  document_id   TEXT REFERENCES documents(id) ON DELETE CASCADE,
  session_id    TEXT REFERENCES sessions(id) ON DELETE CASCADE,
  source        TEXT NOT NULL CHECK (source IN ('transcript','document')),
  text          TEXT NOT NULL,
  embedding     BLOB,
  token_count   INTEGER,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chunks_session ON chunks(session_id);

CREATE TABLE IF NOT EXISTS modes (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  is_builtin       INTEGER NOT NULL DEFAULT 0,
  prompt_template  TEXT NOT NULL,
  custom_context   TEXT,
  created_at_ms    INTEGER NOT NULL,
  updated_at_ms    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

const V2_SCHEMA: &str = r#"
CREATE TABLE transcript_segments_v2 (
  id               TEXT PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  channel          TEXT NOT NULL CHECK (channel IN ('mic','system')),
  origin           TEXT NOT NULL CHECK (origin IN ('local_parakeet','deepgram','speechmatics')),
  text             TEXT NOT NULL,
  start_ms         INTEGER NOT NULL,
  end_ms           INTEGER NOT NULL,
  confidence       REAL,
  is_final         INTEGER NOT NULL DEFAULT 1,
  created_at_ms    INTEGER NOT NULL
);
INSERT INTO transcript_segments_v2
  SELECT id, session_id, channel, origin, text, start_ms, end_ms, confidence, is_final, created_at_ms
  FROM transcript_segments
  WHERE origin IN ('local_parakeet','deepgram','speechmatics');
DROP TABLE transcript_segments;
ALTER TABLE transcript_segments_v2 RENAME TO transcript_segments;
CREATE INDEX IF NOT EXISTS idx_transcript_segments_session ON transcript_segments(session_id, start_ms);
"#;

/// Session-context columns for the Parakeet-style setup screen (2026-08-25):
/// a company name, a job description, a fixed two-value session type, and a
/// per-session auto-answer toggle. `auto_answer` defaults to `1` so every
/// pre-v3 row (and any future insert that doesn't set it explicitly)
/// preserves this project's existing always-fires behavior rather than
/// silently going quiet. Plain `ALTER TABLE ADD COLUMN`, not a
/// rebuild-and-rename like v2 — no existing data needs reshaping, only new
/// nullable/defaulted columns appended.
const V3_SCHEMA: &str = r#"
ALTER TABLE sessions ADD COLUMN company TEXT;
ALTER TABLE sessions ADD COLUMN job_description TEXT;
ALTER TABLE sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'interview' CHECK (session_type IN ('interview','regular_call'));
ALTER TABLE sessions ADD COLUMN auto_answer INTEGER NOT NULL DEFAULT 1;
"#;

/// Accuracy-first memory (2026-08-25): briefing/instructions live off the
/// job-description mash; facts gain a source + priority so STEM OPT from a
/// briefing beats an H-1B line on a resume; interview exchanges remember
/// what the user actually said (never the AI's suggestion); FTS5 sits
/// beside hashed-TF cosine for hybrid retrieval. Append-only — v1–v3 are
/// not edited.
const V4_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_instructions (
  session_id    TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  text          TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

ALTER TABLE facts ADD COLUMN source TEXT NOT NULL DEFAULT 'regex'
  CHECK (source IN ('briefing','spoken_correction','document','previous_session','regex'));
ALTER TABLE facts ADD COLUMN priority INTEGER NOT NULL DEFAULT 10;

CREATE TABLE IF NOT EXISTS interview_exchanges (
  id                    TEXT PRIMARY KEY,
  session_id            TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  interviewer_question  TEXT NOT NULL,
  interviewer_turn_id   TEXT,
  ai_suggestion         TEXT,
  actual_user_answer    TEXT,
  opened_at_ms          INTEGER NOT NULL,
  answered_at_ms        INTEGER,
  suggestion_at_ms      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_interview_exchanges_session
  ON interview_exchanges(session_id, opened_at_ms);

CREATE TABLE IF NOT EXISTS generated_suggestions (
  id            TEXT PRIMARY KEY,
  session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  exchange_id   TEXT REFERENCES interview_exchanges(id) ON DELETE SET NULL,
  question      TEXT NOT NULL,
  suggestion    TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS topic_summaries (
  session_id    TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  text          TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS knowledge_pack_entries (
  id            TEXT PRIMARY KEY,
  session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL CHECK (kind IN ('skill','tech','jd_line','company_term','stt_glossary')),
  value         TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  UNIQUE (session_id, kind, value)
);
CREATE INDEX IF NOT EXISTS idx_knowledge_pack_session
  ON knowledge_pack_entries(session_id, kind);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  text,
  chunk_id UNINDEXED,
  session_id UNINDEXED,
  tokenize = 'porter unicode61'
);
"#;

/// Create/Connect rebuild (2026-08-29, `future refs/live-helper-ui-spec.md`
/// §3.6/§3.5.1): the Create screen's "Save Transcript" checkbox and
/// "Answer Preferences" dropdown, neither of which existed anywhere in the
/// backend before this (confirmed by `grep -rln` across the whole crate).
/// `save_transcript` defaults to `1` for the same reason `auto_answer` does
/// in v3 — every pre-v5 row (and any future insert that doesn't set it
/// explicitly) preserves this project's existing "sessions are saved"
/// behavior rather than silently going quiet. Plain `ALTER TABLE ADD
/// COLUMN`, same shape as v3. **`answer_style` (added here, alongside
/// `save_transcript`) was removed again by [`V7_SCHEMA`]** — the owner
/// asked for the whole Answer Preferences feature ripped out; kept as
/// literal SQL history here rather than edited, since editing what an
/// already-applied migration says would misdescribe every real database
/// that already ran it.
const V5_SCHEMA: &str = r#"
ALTER TABLE sessions ADD COLUMN save_transcript INTEGER NOT NULL DEFAULT 1;
ALTER TABLE sessions ADD COLUMN answer_style TEXT NOT NULL DEFAULT 'default' CHECK (answer_style IN ('default','concise','detailed','coding'));
"#;

/// Fixes the "wrong resume answered" bug: a document uploaded before a
/// session existed (`documents.session_id IS NULL`, the reusable-library
/// case `storage::documents`'s own doc comments describe) was previously
/// selected for a session's prompt context by an oldest-row-wins scan
/// (`documents::list_for_context`) — so a stale old upload could silently
/// beat the one the owner actually picked in the Resume/Documents modal.
/// This table records that explicit pick as its own fact, separate from
/// `documents.session_id` (which keeps meaning "who first uploaded this
/// library entry", unchanged) — `documents::attach_to_session` writes it,
/// `documents::list_attached_for_session` reads it.
const V6_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_documents (
  session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  doc_type    TEXT NOT NULL CHECK (doc_type IN ('resume','job_description','other')),
  attached_at_ms INTEGER NOT NULL,
  PRIMARY KEY (session_id, document_id)
);
CREATE INDEX IF NOT EXISTS idx_session_documents_session ON session_documents(session_id, doc_type);
"#;

/// Owner asked for the "Answer Preferences" feature removed entirely — it
/// was wired into the real prompt only briefly, then the owner decided the
/// whole thing (UI, backend field, prompt behavior) should not exist. Drops
/// the `answer_style` column [`V5_SCHEMA`] added; `save_transcript` (added
/// in the same migration) is untouched and stays. Verified working against
/// this project's bundled SQLite (`rusqlite`'s `bundled` feature): `ALTER
/// TABLE ... DROP COLUMN` on a column with its own inline `CHECK` constraint
/// succeeds — no table rebuild needed, unlike v2.
const V7_SCHEMA: &str = r#"
ALTER TABLE sessions DROP COLUMN answer_style;
"#;

/// Copilots are reusable call setups.  Calls remain separate rows in
/// `sessions`, so a transcript can never mutate the setup that launched it.
/// This migration also turns every surviving document into a library entry
/// before clearing the legacy call history the owner explicitly asked to
/// remove.  Earlier builds stored extracted text only, so that is the only
/// legacy document material that can be preserved.
const V8_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS copilots (
  id              TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  company         TEXT,
  target_role     TEXT,
  job_description TEXT,
  instructions    TEXT,
  auto_answer     INTEGER NOT NULL DEFAULT 0,
  save_transcript INTEGER NOT NULL DEFAULT 1,
  llm_provider    TEXT NOT NULL,
  llm_model       TEXT NOT NULL,
  created_at_ms   INTEGER NOT NULL,
  updated_at_ms   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS copilot_documents (
  copilot_id  TEXT NOT NULL REFERENCES copilots(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  is_resume   INTEGER NOT NULL DEFAULT 0,
  attached_at_ms INTEGER NOT NULL,
  PRIMARY KEY (copilot_id, document_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_copilot_one_resume
  ON copilot_documents(copilot_id) WHERE is_resume = 1;
CREATE INDEX IF NOT EXISTS idx_copilot_documents_copilot
  ON copilot_documents(copilot_id, attached_at_ms DESC);

ALTER TABLE documents ADD COLUMN original_rel_path TEXT;
ALTER TABLE documents ADD COLUMN mime_type TEXT;
ALTER TABLE documents ADD COLUMN byte_size INTEGER;
ALTER TABLE sessions ADD COLUMN copilot_id TEXT REFERENCES copilots(id) ON DELETE SET NULL;
ALTER TABLE sessions ADD COLUMN copilot_snapshot_json TEXT;

UPDATE documents SET session_id = NULL;
DELETE FROM sessions;
"#;

/// One row per completed conversation window's background-generated
/// summary line (2026-08-30, owner request: the Context screen's growing
/// "Conversation so far" list — previously in-memory only, wiped on every
/// session change — needs to survive so a past session's transcript can
/// show it later). `seq` is the append order (0, 1, 2, ...), not derived
/// from `created_at_ms`, since two summaries could in principle finish out
/// of order — `window_memory::background` already guarantees enqueue order
/// separately from completion order.
const V9_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_window_summaries (
  id            TEXT PRIMARY KEY,
  session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  seq           INTEGER NOT NULL,
  text          TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_session_window_summaries_session
  ON session_window_summaries(session_id, seq);
"#;

/// Brings `conn`'s schema up to the latest version. Safe to call on every
/// startup, including against a database that's already current — each step
/// only runs if `user_version` says it hasn't yet, and table creation itself
/// is `IF NOT EXISTS` so a partially-applied run can't leave things broken.
pub fn run(conn: &Connection) -> Result<(), String> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| format!("failed to enable foreign keys: {e}"))?;

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| format!("failed to read schema version: {e}"))?;

    if version < 1 {
        conn.execute_batch(V1_SCHEMA)
            .map_err(|e| format!("migration v1 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 1)
            .map_err(|e| format!("failed to record schema version 1: {e}"))?;
    }

    if version < 2 {
        // Rebuilding a referenced table: foreign keys have to be off for
        // the DROP, then back on so later use matches v1's contract.
        conn.pragma_update(None, "foreign_keys", "OFF")
            .map_err(|e| format!("failed to disable foreign keys for migration v2: {e}"))?;
        conn.execute_batch(V2_SCHEMA)
            .map_err(|e| format!("migration v2 failed: {e}"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| format!("failed to re-enable foreign keys after migration v2: {e}"))?;
        conn.pragma_update(None, "user_version", 2)
            .map_err(|e| format!("failed to record schema version 2: {e}"))?;
    }

    if version < 3 {
        conn.execute_batch(V3_SCHEMA)
            .map_err(|e| format!("migration v3 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 3)
            .map_err(|e| format!("failed to record schema version 3: {e}"))?;
    }

    if version < 4 {
        conn.execute_batch(V4_SCHEMA)
            .map_err(|e| format!("migration v4 failed: {e}"))?;
        // Existing chunks (from before FTS existed) must be searchable too.
        conn.execute(
            "INSERT INTO chunks_fts (text, chunk_id, session_id)
             SELECT text, id, session_id FROM chunks WHERE session_id IS NOT NULL",
            [],
        )
        .map_err(|e| format!("migration v4 FTS backfill failed: {e}"))?;
        conn.pragma_update(None, "user_version", 4)
            .map_err(|e| format!("failed to record schema version 4: {e}"))?;
    }

    if version < 5 {
        conn.execute_batch(V5_SCHEMA)
            .map_err(|e| format!("migration v5 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 5)
            .map_err(|e| format!("failed to record schema version 5: {e}"))?;
    }

    if version < 6 {
        conn.execute_batch(V6_SCHEMA)
            .map_err(|e| format!("migration v6 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 6)
            .map_err(|e| format!("failed to record schema version 6: {e}"))?;
    }

    if version < 7 {
        conn.execute_batch(V7_SCHEMA)
            .map_err(|e| format!("migration v7 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 7)
            .map_err(|e| format!("failed to record schema version 7: {e}"))?;
    }

    if version < 8 {
        conn.execute_batch(V8_SCHEMA)
            .map_err(|e| format!("migration v8 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 8)
            .map_err(|e| format!("failed to record schema version 8: {e}"))?;
    }

    if version < 9 {
        conn.execute_batch(V9_SCHEMA)
            .map_err(|e| format!("migration v9 failed: {e}"))?;
        conn.pragma_update(None, "user_version", 9)
            .map_err(|e| format!("failed to record schema version 9: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cleanly_on_a_fresh_database() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 9);
    }

    #[test]
    fn is_safe_to_run_twice() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        run(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 9);
    }

    #[test]
    fn v9_creates_the_session_window_summaries_table() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='session_window_summaries'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1);
    }

    #[test]
    fn v3_adds_session_context_columns_with_safe_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        // A pre-v3-shaped insert (the exact column list v1 always used) must
        // still work, and the new columns must come back with defaults that
        // preserve this project's existing always-fires auto-answer
        // behavior rather than silently going quiet.
        conn.execute(
            "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms)
             VALUES ('s1', 't', '[]', 0, 'active', 0, 0)",
            [],
        )
        .unwrap();
        let (session_type, auto_answer, company, job_description): (String, i64, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT session_type, auto_answer, company, job_description FROM sessions WHERE id = 's1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(session_type, "interview");
        assert_eq!(auto_answer, 1);
        assert_eq!(company, None);
        assert_eq!(job_description, None);
    }

    #[test]
    fn session_type_check_rejects_an_invalid_value() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let err = conn
            .execute(
                "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms, session_type)
                 VALUES ('s1', 't', '[]', 0, 'active', 0, 0, 'bogus_type')",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK"),
            "an invalid session_type should fail the CHECK constraint, got {err}"
        );
    }

    #[test]
    fn creates_every_v1_table() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        for table in [
            "sessions",
            "transcript_segments",
            "facts",
            "documents",
            "chunks",
            "modes",
            "settings",
            "session_documents",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "expected table `{table}` to exist");
        }
    }

    #[test]
    fn transcript_origin_accepts_the_three_live_engines_and_rejects_anything_else() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms)
             VALUES ('s1', 't', '[]', 0, 'active', 0, 0)",
            [],
        )
        .unwrap();
        for origin in ["local_parakeet", "deepgram", "speechmatics"] {
            conn.execute(
                "INSERT INTO transcript_segments (id, session_id, channel, origin, text, start_ms, end_ms, created_at_ms)
                 VALUES (?1, 's1', 'mic', ?2, 'hi', 0, 1, 0)",
                rusqlite::params![format!("seg-{origin}"), origin],
            )
            .unwrap_or_else(|e| panic!("{origin} must be a valid origin, got {e}"));
        }
        let err = conn
            .execute(
                "INSERT INTO transcript_segments (id, session_id, channel, origin, text, start_ms, end_ms, created_at_ms)
                 VALUES ('bad', 's1', 'mic', 'bogus_engine', 'hi', 0, 1, 0)",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK"),
            "bogus origin should fail the CHECK constraint, got {err}"
        );
    }

    #[test]
    fn v5_adds_save_transcript_column_with_a_safe_default() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        // A pre-v5-shaped insert (the exact column list v1 always used) must
        // still work, and the new column must come back with the default
        // that preserves "sessions are saved".
        conn.execute(
            "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms)
             VALUES ('s1', 't', '[]', 0, 'active', 0, 0)",
            [],
        )
        .unwrap();
        let save_transcript: i64 = conn
            .query_row(
                "SELECT save_transcript FROM sessions WHERE id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(save_transcript, 1);
    }

    #[test]
    fn v7_removes_the_answer_style_column_entirely() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let err = conn
            .query_row("SELECT answer_style FROM sessions LIMIT 1", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("no such column"),
            "answer_style must be gone from the live schema after v7, got {err}"
        );
    }

    #[test]
    fn v8_clears_legacy_calls_without_cascading_away_library_text() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1_SCHEMA).unwrap();
        conn.execute_batch(V2_SCHEMA).unwrap();
        conn.execute_batch(V3_SCHEMA).unwrap();
        conn.execute_batch(V4_SCHEMA).unwrap();
        conn.execute_batch(V5_SCHEMA).unwrap();
        conn.execute_batch(V6_SCHEMA).unwrap();
        conn.execute_batch(V7_SCHEMA).unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute("INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms) VALUES ('old-call', 'Old call', '[]', 1, 'ended', 1, 1)", []).unwrap();
        conn.execute("INSERT INTO documents (id, session_id, doc_type, file_name, raw_text, created_at_ms) VALUES ('resume', 'old-call', 'resume', 'resume.pdf', 'preserved parsed text', 1)", []).unwrap();
        run(&conn).unwrap();
        let calls: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        let (session_id, text): (Option<String>, String) = conn
            .query_row(
                "SELECT session_id, raw_text FROM documents WHERE id='resume'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(calls, 0);
        assert_eq!(session_id, None);
        assert_eq!(text, "preserved parsed text");
    }

    #[test]
    fn v4_creates_memory_tables_and_extends_facts() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        for table in [
            "session_instructions",
            "interview_exchanges",
            "generated_suggestions",
            "topic_summaries",
            "knowledge_pack_entries",
            "chunks_fts",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "expected `{table}` after v4");
        }
        conn.execute(
            "INSERT INTO sessions (id, title, participants, started_at_ms, status, created_at_ms, updated_at_ms)
             VALUES ('s1', 't', '[]', 0, 'active', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO facts (id, session_id, key, value, updated_at_ms)
             VALUES ('f1', 's1', 'visa_status', 'H-1B', 0)",
            [],
        )
        .unwrap();
        let (source, priority): (String, i64) = conn
            .query_row(
                "SELECT source, priority FROM facts WHERE id = 'f1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "regex");
        assert_eq!(priority, 10);
    }
}
