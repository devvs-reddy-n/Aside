//! Hybrid retrieval: SQLite FTS5 + hashed TF cosine, RRF k=60.
//! Session-filtered. No Pinecone, sqlite-vec, cross-encoder, LLM rerank,
//! or network embeddings on the live path. 80ms deadline.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use uuid::Uuid;

use crate::rag::retriever::{self, ScoredChunk};
use crate::storage::chunks;

pub const RRF_K: u32 = 60;
pub const DEADLINE: Duration = Duration::from_millis(80);
pub const TARGET_CHUNKS: usize = 50;

fn rrf(rank: usize) -> f64 {
    1.0 / (f64::from(RRF_K) + rank as f64 + 1.0)
}

fn looks_like_visa_or_project(query: &str) -> bool {
    let l = query.to_lowercase();
    l.contains("visa")
        || l.contains("opt")
        || l.contains("h-1b")
        || l.contains("h1b")
        || l.contains("sponsorship")
        || l.contains("project")
        || l.contains("notice period")
}

/// Ranked chunk ids for this session. Cosine uses already-loaded vectors
/// (no new embedder init here — the caller supplies `query_vec`).
pub fn retrieve(
    conn: &Connection,
    session_id: Uuid,
    query: &str,
    query_vec: Option<&[f32]>,
    candidates: &[(Uuid, String, Vec<f32>)],
    started: Instant,
) -> Vec<(Uuid, String)> {
    let by_id: HashMap<Uuid, &str> = candidates
        .iter()
        .map(|(id, text, _)| (*id, text.as_str()))
        .collect();

    let fts_ids = chunks::search_fts(conn, session_id, query, TARGET_CHUNKS).unwrap_or_default();

    if started.elapsed() > DEADLINE {
        return fts_ids
            .into_iter()
            .filter_map(|id| by_id.get(&id).map(|t| (id, (*t).to_string())))
            .take(8)
            .collect();
    }

    let mut cosine_ids: Vec<Uuid> = Vec::new();
    if let Some(qv) = query_vec {
        let for_scoring: Vec<(Uuid, Vec<f32>)> = candidates
            .iter()
            .map(|(id, _, vec)| (*id, vec.clone()))
            .collect();
        let scored: Vec<ScoredChunk> = retriever::top_k(qv, &for_scoring, TARGET_CHUNKS, 0.1);
        cosine_ids = scored.into_iter().map(|s| s.chunk_id).collect();
    }

    if looks_like_visa_or_project(query) && !fts_ids.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for id in fts_ids.iter().chain(cosine_ids.iter()) {
            if seen.insert(*id) {
                if let Some(t) = by_id.get(id) {
                    out.push((*id, (*t).to_string()));
                }
            }
            if out.len() >= 8 {
                break;
            }
        }
        return out;
    }

    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for (rank, id) in fts_ids.iter().enumerate() {
        *scores.entry(*id).or_insert(0.0) += rrf(rank);
    }
    for (rank, id) in cosine_ids.iter().enumerate() {
        *scores.entry(*id).or_insert(0.0) += rrf(rank);
    }
    let mut ranked: Vec<(Uuid, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
        .into_iter()
        .filter_map(|(id, _)| by_id.get(&id).map(|t| (id, (*t).to_string())))
        .take(8)
        .collect()
}

pub fn texts_only(hits: Vec<(Uuid, String)>) -> Vec<String> {
    hits.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::rag::embed_tfidf::vectorize;
    use crate::storage::{chunks as chunk_store, sessions, Db};
    use std::time::Instant;

    #[test]
    fn hybrid_over_fifty_chunks_stays_under_thirty_ms() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        let mut candidates = Vec::new();
        for i in 0..50 {
            let text = if i == 7 {
                "Authorized to work in the United States on STEM OPT".to_string()
            } else {
                format!("project work item {i} rust backend systems design")
            };
            let vec = vectorize(&text);
            let id = chunk_store::insert_chunk(
                &conn,
                session.id,
                None,
                "document",
                &text,
                Some(&vec),
                None,
                i as u64,
            )
            .unwrap();
            candidates.push((id, text, vec));
        }
        let q = "what is your visa status STEM OPT";
        let qv = vectorize(q);
        let started = Instant::now();
        let hits = retrieve(&conn, session.id, q, Some(&qv), &candidates, started);
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 30,
            "hybrid over 50 chunks took {}ms, budget is 30ms",
            elapsed.as_millis()
        );
        let joined = hits
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.contains("STEM OPT"),
            "visa query should surface the OPT chunk, got {joined}"
        );
    }
}
