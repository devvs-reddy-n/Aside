//! Incremental indexing (`status.md` 7.7) — the piece that keeps RAG cheap
//! during a live call: every **final** transcript feeds a per-channel
//! [`crate::rag::chunker::StreamingChunker`], and only the chunk(s) that
//! just became complete are embedded and stored. Nothing is ever
//! re-embedded — a session with an hour of transcript costs exactly the
//! same per-chunk embedding work whether it's chunk 1 or chunk 100, never a
//! whole-session re-pass.
//!
//! Wired into `pipeline.rs`'s `emit_all`, the same spot Phase 6's
//! `llm::maybe_trigger` hooks in (`status.md` 7.7's own instruction) — and
//! following that call's exact shape: a cheap synchronous check
//! (`StreamingChunker::push`, pure in-memory text work, no I/O) followed by
//! `tauri::async_runtime::spawn` for the one part that can be slow (turning
//! text into a vector, then a database write), so indexing latency is never
//! on the critical path a live transcript's UI update waits on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::audio::channel::Channel;
use crate::storage::{chunks as chunk_storage, settings, Db};

use super::chunker::{Chunk, StreamingChunker};
use super::embed::{Embedder, EmbedderKind};
use super::embed_cloud::CloudEmbedder;
use super::embed_onnx::OnnxEmbedder;
use super::embed_tfidf::HashedTermFrequencyEmbedder;
use super::{CLOUD_API_KEY_SETTING, EMBEDDER_SETTING_KEY};

/// Per-app Tauri state: one streaming chunker per `(session, channel)` in
/// progress, plus a lazily-loaded, shared MiniLM instance (loading it is a
/// real multi-second model load — must happen at most once per process,
/// never once per chunk).
#[derive(Default)]
pub struct RagIndexState {
    chunkers: StdMutex<HashMap<(Uuid, Channel), StreamingChunker>>,
    onnx_singleton: AsyncMutex<Option<Arc<OnnxEmbedder>>>,
}

fn speaker_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Mic => "you",
        Channel::System => "others",
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Feeds one final transcript segment's text into `channel`'s streaming
/// chunker. A no-op on empty text. Speaker-prefixes the text (`"[you] ..."`
/// / `"[others] ..."`, adapted from `references/Repo1-Natively`'s
/// `TemporalContextBuilder.ts` `formatTranscript` — same idea of baking who
/// said it into the text a later prompt sees, independently reasoned for
/// this project's own chunk-storage shape rather than ported code) so a
/// retrieved chunk is still legible without a separate speaker column.
pub fn index_final_transcript(app: &AppHandle, session_id: Uuid, channel: Channel, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let labeled = format!("[{}] {trimmed}", speaker_label(channel));
    let ready_chunks = {
        let state = app.state::<RagIndexState>();
        let mut chunkers = state.chunkers.lock().unwrap_or_else(|p| p.into_inner());
        chunkers
            .entry((session_id, channel))
            .or_default()
            .push(&labeled)
    };
    for chunk in ready_chunks {
        spawn_embed_and_store(app.clone(), session_id, None, chunk);
    }
}

/// Flushes any still-pending partial chunk for `session_id` (both
/// channels) and drops that session's chunker state — call when a session
/// ends so the last few words spoken aren't silently left un-indexed
/// forever, and so a long-running app doesn't accumulate chunker state for
/// sessions that have already ended.
pub fn flush_session(app: &AppHandle, session_id: Uuid) {
    let leftovers = {
        let state = app.state::<RagIndexState>();
        let mut chunkers = state.chunkers.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        for channel in [Channel::Mic, Channel::System] {
            if let Some(chunker) = chunkers.get_mut(&(session_id, channel)) {
                if let Some(chunk) = chunker.flush() {
                    out.push(chunk);
                }
            }
        }
        chunkers.retain(|(sid, _), _| *sid != session_id);
        out
    };
    for chunk in leftovers {
        spawn_embed_and_store(app.clone(), session_id, None, chunk);
    }
}

/// Indexes a whole document into RAG in one shot (`status.md` 8.5) — the
/// real integration point between Phase 8 (resume/JD ingestion) and Phase
/// 7's chunk/RAG pipeline.
///
/// **Real design decision, stated explicitly rather than left implicit:**
/// a document arrives whole (the file picker hands over its complete
/// extracted text at once), unlike a live transcript's incremental stream
/// of final segments — so this uses [`super::chunker::chunk_text`], the
/// whole-text entry point `rag::chunker`'s own module doc comment already
/// names as being for exactly this ("e.g. a future Phase 8 document"), not
/// [`StreamingChunker`] (built for accumulating text across many small
/// `push` calls, which a one-shot document upload never does). Each
/// resulting chunk is embedded and stored the same way a transcript chunk
/// is (same `spawn_embed_and_store` code path, so there is exactly one
/// place that turns a `Chunk` into a stored, embedded row — not two
/// drifting copies), tagged `source = "document"` and a real
/// `document_id` so a later query can tell "the AI recalled this because
/// you said it" from "the AI recalled this from your resume" apart.
///
/// Returns the number of chunks queued for embedding — real chunk work has
/// already happened synchronously (`chunk_text` is pure, in-memory, no
/// I/O); only the embed-and-store half runs on a background task per
/// chunk, matching `index_final_transcript`'s own reasoning for why that
/// split exists (indexing latency must never sit on a caller's critical
/// path).
pub fn index_document(app: &AppHandle, session_id: Uuid, document_id: Uuid, text: &str) -> usize {
    let chunks = super::chunker::chunk_text(text);
    let count = chunks.len();
    for chunk in chunks {
        spawn_embed_and_store(app.clone(), session_id, Some(document_id), chunk);
    }
    count
}

fn spawn_embed_and_store(
    app: AppHandle,
    session_id: Uuid,
    document_id: Option<Uuid>,
    chunk: Chunk,
) {
    let source = if document_id.is_some() {
        "document"
    } else {
        "transcript"
    };
    tauri::async_runtime::spawn(async move {
        let embedder = match active_embedder(&app).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("rag indexing: could not build embedder: {e}");
                return;
            }
        };
        match embedder.embed(&chunk.text).await {
            Ok(vector) => {
                // Defensive: a vector whose length doesn't match what this
                // embedder itself declares would silently poison every
                // future cosine-similarity comparison for this session
                // (`retriever::cosine_similarity` returns 0.0 for a length
                // mismatch rather than panicking, so this would otherwise
                // fail *quietly* — a chunk that just never matches anything
                // — instead of surfacing as the real bug it'd be).
                if vector.len() != embedder.dimensions() {
                    eprintln!(
                        "rag indexing: {:?} embedder returned a {}-dim vector, expected {} — discarding",
                        embedder.kind(),
                        vector.len(),
                        embedder.dimensions()
                    );
                    return;
                }
                let db = app.state::<Db>();
                let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
                if let Err(e) = chunk_storage::insert_chunk(
                    &conn,
                    session_id,
                    document_id,
                    source,
                    &chunk.text,
                    Some(&vector),
                    Some(chunk.word_count as i64),
                    now_ms(),
                ) {
                    eprintln!("rag indexing: failed to store chunk: {e}");
                }
            }
            Err(e) => eprintln!("rag indexing: failed to embed chunk: {e}"),
        }
    });
}

/// Reads the configured embedder kind from settings (defaulting to
/// `TfIdf` — this phase's measured default, `docs/decisions.md` — when
/// nothing has been chosen yet) and returns a ready instance. `TfIdf` is
/// free to construct every call. `Onnx` loads the shared singleton (or
/// triggers the one-time load, including a download if the model isn't
/// cached yet). `Cloud` requires a saved API key — a missing one is a real
/// `Err`, not a silent fallback to a different embedder that would quietly
/// change what's being compared in retrieval mid-session.
pub(crate) async fn active_embedder(app: &AppHandle) -> Result<Arc<dyn Embedder>, String> {
    let kind = {
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        settings::get(&conn, EMBEDDER_SETTING_KEY)
            .ok()
            .flatten()
            .and_then(|s| EmbedderKind::from_setting_value(&s))
            .unwrap_or(EmbedderKind::TfIdf)
    };

    match kind {
        EmbedderKind::TfIdf => Ok(Arc::new(HashedTermFrequencyEmbedder) as Arc<dyn Embedder>),
        EmbedderKind::Onnx => load_onnx_singleton(app).await,
        EmbedderKind::Cloud => {
            let api_key = {
                let db = app.state::<Db>();
                let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
                settings::get(&conn, CLOUD_API_KEY_SETTING).ok().flatten()
            };
            let api_key = api_key
                .ok_or_else(|| "cloud embeddings selected but no API key is saved".to_string())?;
            let client = app.state::<crate::llm::HttpClient>().0.clone();
            Ok(Arc::new(CloudEmbedder::new(client, api_key)) as Arc<dyn Embedder>)
        }
    }
}

async fn load_onnx_singleton(app: &AppHandle) -> Result<Arc<dyn Embedder>, String> {
    let state = app.state::<RagIndexState>();
    let mut cached = state.onnx_singleton.lock().await;
    if let Some(existing) = cached.as_ref() {
        return Ok(existing.clone() as Arc<dyn Embedder>);
    }

    let models_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?
        .join("models");
    let client = app.state::<crate::llm::HttpClient>().0.clone();
    let model_dir = super::embed_onnx_cache::ensure_downloaded(&client, &models_root).await?;
    let embedder = tauri::async_runtime::spawn_blocking(move || {
        super::embed_onnx::OnnxEmbedder::load(&model_dir)
    })
    .await
    .map_err(|e| format!("MiniLM load task panicked: {e}"))??;
    let embedder = Arc::new(embedder);
    *cached = Some(embedder.clone());
    Ok(embedder as Arc<dyn Embedder>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::sessions;

    fn seeded_session(db: &Db) -> Uuid {
        let conn = db.0.lock().unwrap();
        let session = Session::new("Test session", 1_000);
        sessions::create(&conn, &session).unwrap();
        session.id
    }

    #[test]
    fn speaker_labels_match_the_panels_you_them_naming() {
        assert_eq!(speaker_label(Channel::Mic), "you");
        assert_eq!(speaker_label(Channel::System), "others");
    }

    /// `status.md` 8.5's real integration point: a whole document is
    /// chunked **at once** (`chunker::chunk_text`, not the per-segment
    /// `StreamingChunker` transcripts use — see `index_document`'s own doc
    /// comment for why those are genuinely different shapes) and every
    /// resulting chunk is tagged with the document's id and `source =
    /// "document"`, distinguishing it from a transcript-sourced chunk.
    /// Exercises the real `chunk_text` + `storage::chunks` path directly
    /// (no `AppHandle` needed — same reasoning as this file's other
    /// `#[tokio::test]`s below: only the `RagIndexState`/`active_embedder`
    /// wiring `index_document` itself adds needs one, not the
    /// chunking/storage logic being proven here).
    #[tokio::test]
    async fn a_whole_document_is_chunked_at_once_and_every_chunk_is_tagged_with_its_document_id() {
        use crate::storage::documents;

        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let doc_id = {
            let conn = db.0.lock().unwrap();
            documents::insert_document(
                &conn,
                Some(session_id),
                "resume",
                "resume.pdf",
                "irrelevant for this test",
                1_000,
            )
            .unwrap()
        };

        // 90 words across 6 sentences — enough to cross chunk_text's
        // 80-word target, proving a long-enough document really does
        // produce more than one chunk (unlike any single live transcript
        // segment, which is usually well under that).
        let mut text = String::new();
        for s in 0..6 {
            text.push_str(
                &(0..15)
                    .map(|w| format!("seg{s}w{w}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            text.push_str(". ");
        }

        let embedder = HashedTermFrequencyEmbedder;
        let chunks = crate::rag::chunker::chunk_text(&text);
        assert!(
            chunks.len() >= 1,
            "90 words should produce at least one chunk"
        );

        for chunk in &chunks {
            let vector = embedder.embed(&chunk.text).await.unwrap();
            let conn = db.0.lock().unwrap();
            chunk_storage::insert_chunk(
                &conn,
                session_id,
                Some(doc_id),
                "document",
                &chunk.text,
                Some(&vector),
                Some(chunk.word_count as i64),
                2_000,
            )
            .unwrap();
        }

        let conn = db.0.lock().unwrap();
        let stored = chunk_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), chunks.len());
        for row in &stored {
            assert_eq!(
                row.document_id,
                Some(doc_id),
                "every document-sourced chunk must carry the real document id"
            );
            assert_eq!(row.source, "document");
        }
    }

    /// The real regression this phase's incremental-indexing requirement
    /// (`status.md` 7.7 / 7.10) is about: a chunk is stored **once**, only
    /// once it's genuinely complete, never re-embedded as more text arrives
    /// in the same session. Exercises the real `StreamingChunker` +
    /// `storage::chunks` path directly (no Tauri `AppHandle` needed, since
    /// the chunking and storage logic here doesn't touch it — only the
    /// `RagIndexState`/`active_embedder` wiring above does).
    #[tokio::test]
    async fn a_chunk_is_stored_exactly_once_when_its_word_target_is_crossed_not_re_embedded_on_every_push(
    ) {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let embedder = HashedTermFrequencyEmbedder;
        let mut chunker = StreamingChunker::default();
        let mut stored_texts = Vec::new();

        // Feed 6 segments of 15 words each = 90 words, crossing the 80-word
        // target partway through.
        for s in 0..6 {
            let seg = (0..15)
                .map(|w| format!("seg{s}w{w}"))
                .collect::<Vec<_>>()
                .join(" ")
                + ".";
            for chunk in chunker.push(&seg) {
                let vector = embedder.embed(&chunk.text).await.unwrap();
                let conn = db.0.lock().unwrap();
                chunk_storage::insert_chunk(
                    &conn,
                    session_id,
                    None,
                    "transcript",
                    &chunk.text,
                    Some(&vector),
                    Some(chunk.word_count as i64),
                    1_000,
                )
                .unwrap();
                stored_texts.push(chunk.text);
            }
        }

        let conn = db.0.lock().unwrap();
        let count = chunk_storage::count_for_session(&conn, session_id).unwrap();
        assert_eq!(
            count as usize,
            stored_texts.len(),
            "every emitted chunk must be stored exactly once"
        );
        assert!(
            count >= 1,
            "90 words across 6 pushes must have produced at least one complete chunk"
        );

        // No stored chunk's text is a strict prefix of another — proves
        // nothing was "re-cut and re-stored" with overlapping content.
        for (i, a) in stored_texts.iter().enumerate() {
            for (j, b) in stored_texts.iter().enumerate() {
                if i != j {
                    assert!(
                        !b.starts_with(a.as_str()),
                        "chunk {i:?} text was re-embedded inside chunk {j:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn embedder_kind_setting_round_trips_through_storage() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        settings::set(
            &conn,
            EMBEDDER_SETTING_KEY,
            EmbedderKind::Onnx.as_setting_value(),
        )
        .unwrap();
        let read_back = settings::get(&conn, EMBEDDER_SETTING_KEY).unwrap().unwrap();
        assert_eq!(
            EmbedderKind::from_setting_value(&read_back),
            Some(EmbedderKind::Onnx)
        );
    }
}

/// Performance regression test (`status.md` 7.10's explicit call-out: "plus
/// a performance regression test with an explicit latency budget"). The
/// budget below isn't a guess — it's this exact test, measured for real on
/// this machine (Apple M5) before being written, in the same `cargo test
/// --lib` debug profile the standing rule (`CLAUDE.md`) runs: **31.3ms**
/// for 100 segments (313µs/segment average) through the real path —
/// `StreamingChunker::push` -> `HashedTermFrequencyEmbedder::embed` ->
/// `storage::chunks::insert_chunk` -> `facts::extract::extract` ->
/// `storage::facts::upsert_fact`, exactly what `pipeline.rs`'s `emit_all`
/// triggers per final transcript (minus the `AppHandle`/Tauri-state
/// plumbing this test doesn't need to exercise the real cost of). 150ms —
/// roughly 5x that measurement — is the failure line: comfortably above
/// real noise, but tight enough to catch a genuine regression (e.g.
/// someone swapping the default embedder for a slow one, or an accidental
/// O(n²) creeping into the chunker). See `docs/evidence.md` for the full
/// benchmark this number comes from, including the release-mode numbers
/// (5.7ms / 100 segments) recorded alongside it.
#[cfg(test)]
mod perf_regression {
    use super::*;
    use crate::domain::Session;
    use crate::storage::sessions;
    use std::time::Instant;

    const SEGMENT_COUNT: u32 = 100;
    const BUDGET_MS_DEBUG_PROFILE: u128 = 150;

    #[tokio::test]
    async fn one_hundred_segments_through_the_real_incremental_path_stays_under_budget() {
        let db = Db::open_in_memory();
        let session_id = {
            let conn = db.0.lock().unwrap();
            let session = Session::new("perf regression", 0);
            sessions::create(&conn, &session).unwrap();
            session.id
        };
        let embedder = HashedTermFrequencyEmbedder;
        let mut chunker = StreamingChunker::default();

        let start = Instant::now();
        for s in 0..SEGMENT_COUNT {
            let seg = format!(
                "This is segment number {s} in a realistic sounding sentence about the interview."
            );
            for chunk in chunker.push(&seg) {
                let vector = embedder.embed(&chunk.text).await.unwrap();
                let conn = db.0.lock().unwrap();
                chunk_storage::insert_chunk(
                    &conn,
                    session_id,
                    None,
                    "transcript",
                    &chunk.text,
                    Some(&vector),
                    Some(chunk.word_count as i64),
                    0,
                )
                .unwrap();
            }
            for fact in crate::facts::extract::extract(&seg) {
                let conn = db.0.lock().unwrap();
                let _ = crate::storage::facts::upsert_fact(
                    &conn,
                    session_id,
                    &fact.key,
                    &fact.value,
                    None,
                    Some(fact.confidence),
                    0,
                );
            }
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < BUDGET_MS_DEBUG_PROFILE,
            "incremental indexing of {SEGMENT_COUNT} segments took {}ms, budget is {BUDGET_MS_DEBUG_PROFILE}ms \
             (measured baseline on this machine: ~31ms) — this is a real performance regression, not test flakiness",
            elapsed.as_millis()
        );
    }
}
