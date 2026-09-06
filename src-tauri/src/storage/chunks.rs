//! RAG chunks — the embedded ~80-word pieces `rag::retriever` searches
//! (`status.md` 7.9). Follows `storage/transcripts.rs`'s established shape
//! exactly, per that module's own doc comment (plain functions over a
//! `&Connection`, a row-mapping helper, `String`-formatted errors at the
//! storage boundary) — the freshest example of this project's storage
//! convention at the time this was written.
//!
//! `embedding` is stored as the schema's `BLOB` column: a `Vec<f32>`
//! serialized as raw little-endian bytes (`f32::to_le_bytes`), not JSON —
//! this is a hot read path (every retrieval query loads every chunk's
//! vector for a session) and a 384-dim `f32` vector is 1536 bytes as a
//! binary blob versus several times that as a JSON array of decimal
//! strings, with no parsing cost on the way back out.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredChunk {
    pub id: Uuid,
    pub session_id: Uuid,
    pub document_id: Option<Uuid>,
    pub source: String,
    pub text: String,
    pub embedding: Option<Vec<f32>>,
    pub token_count: Option<i64>,
    pub created_at_ms: u64,
}

/// `Vec<f32>` -> raw little-endian bytes.
pub fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for v in vector {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Raw little-endian bytes -> `Vec<f32>`. Silently drops a trailing partial
/// float (a corrupt/truncated blob) rather than erroring — a chunk with a
/// slightly damaged vector should just score badly in retrieval, not break
/// every query for the whole session.
pub fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn row_to_chunk(row: &rusqlite::Row) -> rusqlite::Result<StoredChunk> {
    let id: String = row.get("id")?;
    let session_id: String = row.get("session_id")?;
    let document_id: Option<String> = row.get("document_id")?;
    let embedding_blob: Option<Vec<u8>> = row.get("embedding")?;
    Ok(StoredChunk {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&session_id).unwrap_or_else(|_| Uuid::nil()),
        document_id: document_id.and_then(|d| Uuid::parse_str(&d).ok()),
        source: row.get("source")?,
        text: row.get("text")?,
        embedding: embedding_blob.map(|b| decode_embedding(&b)),
        token_count: row.get("token_count")?,
        created_at_ms: row.get("created_at_ms")?,
    })
}

/// Inserts one chunk and returns its generated id. `document_id` is `NULL`
/// for transcript-sourced chunks (Phase 7's original, still-only-caller
/// shape) and `Some(id)` for Phase 8's document-sourced chunks — the real
/// caller `insert_chunk`'s own doc comment used to promise was coming.
#[allow(clippy::too_many_arguments)]
pub fn insert_chunk(
    conn: &Connection,
    session_id: Uuid,
    document_id: Option<Uuid>,
    source: &str,
    text: &str,
    embedding: Option<&[f32]>,
    token_count: Option<i64>,
    created_at_ms: u64,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO chunks (id, document_id, session_id, source, text, embedding, token_count, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id.to_string(),
            document_id.map(|d| d.to_string()),
            session_id.to_string(),
            source,
            text,
            embedding.map(encode_embedding),
            token_count,
            created_at_ms,
        ],
    )
    .map_err(|e| format!("failed to insert chunk: {e}"))?;
    let _ = conn.execute(
        "INSERT INTO chunks_fts (text, chunk_id, session_id) VALUES (?1, ?2, ?3)",
        params![text, id.to_string(), session_id.to_string()],
    );
    Ok(id)
}

/// All chunks for one session, oldest first — the candidate pool
/// `rag::retriever::top_k` scores against, called from
/// `llm::retrieve_context` at answer time.
#[cfg(test)]
pub fn list_for_session(conn: &Connection, session_id: Uuid) -> Result<Vec<StoredChunk>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, document_id, source, text, embedding, token_count, created_at_ms
             FROM chunks WHERE session_id = ?1 ORDER BY created_at_ms ASC",
        )
        .map_err(|e| format!("failed to prepare chunk list query: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.to_string()], row_to_chunk)
        .map_err(|e| format!("failed to run chunk list query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read chunk row: {e}"))
}

#[allow(dead_code)] // real query API for a future consumer (a "what did we index" debug view)
pub fn get(conn: &Connection, id: Uuid) -> Result<Option<StoredChunk>, String> {
    conn.query_row(
        "SELECT id, session_id, document_id, source, text, embedding, token_count, created_at_ms
         FROM chunks WHERE id = ?1",
        params![id.to_string()],
        row_to_chunk,
    )
    .optional()
    .map_err(|e| format!("failed to load chunk: {e}"))
}

/// How many chunks a session already has — the incremental-indexing
/// correctness proof reads this before/after a partial-text push to confirm
/// nothing was inserted until a chunk was genuinely complete.
#[cfg(test)]
pub fn count_for_session(conn: &Connection, session_id: Uuid) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM chunks WHERE session_id = ?1",
        params![session_id.to_string()],
        |r| r.get(0),
    )
    .map_err(|e| format!("failed to count chunks: {e}"))
}

/// FTS5 lexical hits for this session, in rank order. Empty query → empty vec.
pub fn search_fts(
    conn: &Connection,
    session_id: Uuid,
    query: &str,
    limit: usize,
) -> Result<Vec<Uuid>, String> {
    let match_q = fts_match_query(query);
    if match_q.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT chunk_id FROM chunks_fts
             WHERE chunks_fts MATCH ?1 AND session_id = ?2
             LIMIT ?3",
        )
        .map_err(|e| format!("failed to prepare FTS query: {e}"))?;
    let rows = stmt
        .query_map(
            params![match_q, session_id.to_string(), limit as i64],
            |row| {
                let id: String = row.get(0)?;
                Ok(Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()))
            },
        )
        .map_err(|e| format!("failed to run FTS query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read FTS row: {e}"))
}

/// Quote alphanumeric tokens so a user question cannot break MATCH syntax.
pub fn fts_match_query(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() >= 2)
        .take(16)
        .map(|w| {
            let cleaned: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect();
            format!("\"{cleaned}\"")
        })
        .filter(|w| w.len() > 2)
        .collect::<Vec<_>>()
        .join(" OR ")
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
    fn embedding_bytes_round_trip_exactly() {
        let original = vec![0.1f32, -0.5, 3.25, 0.0, -1.0];
        let bytes = encode_embedding(&original);
        assert_eq!(bytes.len(), original.len() * 4);
        let decoded = decode_embedding(&bytes);
        assert_eq!(decoded, original);
    }

    #[test]
    fn insert_then_get_round_trips_every_field_including_the_embedding() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();

        let vector = vec![1.0f32, 2.0, 3.0];
        let id = insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "hello there",
            Some(&vector),
            Some(3),
            1_000,
        )
        .unwrap();

        let loaded = get(&conn, id).unwrap().expect("chunk should exist");
        assert_eq!(loaded.session_id, session_id);
        assert_eq!(loaded.document_id, None);
        assert_eq!(loaded.source, "transcript");
        assert_eq!(loaded.text, "hello there");
        assert_eq!(loaded.embedding, Some(vector));
        assert_eq!(loaded.token_count, Some(3));
    }

    #[test]
    fn a_chunk_with_no_embedding_round_trips_as_none() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        let id = insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "not yet embedded",
            None,
            None,
            1_000,
        )
        .unwrap();
        assert_eq!(get(&conn, id).unwrap().unwrap().embedding, None);
    }

    /// `status.md` 8.5's real integration point: a document-sourced chunk
    /// carries a real `document_id`, distinguishing it from Phase 7's
    /// transcript-sourced chunks (which always pass `None`).
    #[test]
    fn a_document_sourced_chunk_round_trips_with_its_document_id() {
        use crate::storage::documents;

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        let doc_id = documents::insert_document(
            &conn,
            Some(session_id),
            "resume",
            "resume.pdf",
            "5 years of Python",
            500,
        )
        .unwrap();
        let chunk_id = insert_chunk(
            &conn,
            session_id,
            Some(doc_id),
            "document",
            "5 years of Python",
            None,
            Some(4),
            600,
        )
        .unwrap();

        let loaded = get(&conn, chunk_id).unwrap().expect("chunk should exist");
        assert_eq!(loaded.document_id, Some(doc_id));
        assert_eq!(loaded.source, "document");
    }

    #[test]
    fn list_for_session_orders_oldest_first() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "third",
            None,
            None,
            3_000,
        )
        .unwrap();
        insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "first",
            None,
            None,
            1_000,
        )
        .unwrap();
        insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "second",
            None,
            None,
            2_000,
        )
        .unwrap();
        let chunks = list_for_session(&conn, session_id).unwrap();
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn sessions_do_not_leak_chunks_into_each_others_lists() {
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        insert_chunk(
            &conn,
            session_a,
            None,
            "transcript",
            "a's chunk",
            None,
            None,
            100,
        )
        .unwrap();
        insert_chunk(
            &conn,
            session_b,
            None,
            "transcript",
            "b's chunk",
            None,
            None,
            100,
        )
        .unwrap();
        assert_eq!(list_for_session(&conn, session_a).unwrap().len(), 1);
        assert_eq!(list_for_session(&conn, session_b).unwrap().len(), 1);
    }

    #[test]
    fn count_for_session_reflects_real_inserts() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        assert_eq!(count_for_session(&conn, session_id).unwrap(), 0);
        insert_chunk(
            &conn,
            session_id,
            None,
            "transcript",
            "one",
            None,
            None,
            100,
        )
        .unwrap();
        assert_eq!(count_for_session(&conn, session_id).unwrap(), 1);
    }

    #[test]
    fn fts_finds_a_chunk_in_this_session_only() {
        let db = Db::open_in_memory();
        let session_a = seeded_session(&db);
        let session_b = seeded_session(&db);
        let conn = db.0.lock().unwrap();
        let id = insert_chunk(
            &conn,
            session_a,
            None,
            "document",
            "Authorized to work in the US on STEM OPT",
            None,
            None,
            100,
        )
        .unwrap();
        insert_chunk(
            &conn,
            session_b,
            None,
            "document",
            "Authorized to work in the US on STEM OPT",
            None,
            None,
            100,
        )
        .unwrap();
        let hits = search_fts(&conn, session_a, "STEM OPT visa", 10).unwrap();
        assert_eq!(hits, vec![id]);
    }
}
