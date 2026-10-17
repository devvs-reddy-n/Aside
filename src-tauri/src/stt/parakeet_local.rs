//! Local speech-to-text via NVIDIA Parakeet Realtime EOU 120M v1 (English
//! ONNX), through `parakeet-rs` 0.3.7 with default (CPU) features.
//!
//! This is a true streaming engine: each 160 ms chunk runs cache-aware
//! encoder/decoder steps and returns **text deltas**. End of utterance is
//! the model's own `<EOU>` token (surfaced as `" [EOU]"` when
//! `reset_on_eou` is true) — Aside does **not** use the speaking-light VAD
//! to cut local transcripts. LocalAgreement-2 is not used here: that
//! stabilizer exists to calm flicker from overlapping full-buffer
//! re-inference, which this path no longer does.
//!
//! **Must never run on a real-time audio callback thread.** `transcribe` is
//! a synchronous `&mut self` call. Every invocation goes through
//! `tauri::async_runtime::spawn_blocking`.
//!
//! **One copy of the ~480 MB weights.** Mic and system audio each get their
//! own stream state (`ParakeetEOU`) but share one loaded handle. When the
//! last engine drops, the weights are freed.
//!
//! **Acceleration, stated honestly:** default CPU execution. `parakeet-rs`
//! documents CoreML as unstable/slower; we do not enable that feature.
//! The crate waits until ~1 s of audio is buffered before the first decode
//! (its `MIN_BUFFER_SAMPLES`) — after that, chunks emit as they arrive.
//! No punctuation/capitalization from this model.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use parakeet_rs::{ParakeetEOU, ParakeetEOUHandle};

use super::hallucination_filter;
use super::resample::{resample_linear, STT_SAMPLE_RATE};
use super::{SttEngine, Transcript};

/// 160 ms at 16 kHz — the chunk size `parakeet-rs`'s EOU example feeds.
pub const CHUNK_SAMPLES: usize = 2560;

/// Silence pads used on `flush` so the decoder can emit a trailing EOU,
/// matching the crate's streaming example (3 × 160 ms of zeros).
const FLUSH_SILENCE_CHUNKS: usize = 3;

type SharedHandle = Arc<ParakeetEOUHandle>;

fn shared_slot() -> &'static Mutex<Weak<ParakeetEOUHandle>> {
    static SLOT: OnceLock<Mutex<Weak<ParakeetEOUHandle>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Weak::new()))
}

pub struct ParakeetLocalEngine {
    /// Already resampled to 16 kHz on push — inference never needs the
    /// source rate. Holds a remainder shorter than one EOU chunk.
    pending: Vec<f32>,
    /// Growing deltas for the open utterance (preview / final source).
    utterance: String,
    /// Held so `shared_slot`'s `Weak` stays upgradeable while any engine is
    /// live. The stream also holds ONNX Arcs, but those are a different type
    /// — dropping this `Arc` would make the next channel load a second copy
    /// of the weights. Never read except by the shared-load test.
    #[allow(dead_code)]
    handle: SharedHandle,
    stream: Arc<Mutex<ParakeetEOU>>,
}

impl ParakeetLocalEngine {
    /// Loads the ONNX model from `model_dir` (the folder that holds
    /// `encoder.onnx`, `decoder_joint.onnx`, and `tokenizer.json`).
    /// Blocking — call from an async context via `spawn_blocking`, not from
    /// any real-time thread. A second call while another channel already
    /// holds a live engine reuses the same weights.
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let handle = get_or_load_shared(model_dir)?;
        let mut stream = ParakeetEOU::from_shared(&handle);
        prime_stream(&mut stream);
        Ok(Self {
            pending: Vec::new(),
            utterance: String::new(),
            handle,
            stream: Arc::new(Mutex::new(stream)),
        })
    }

    async fn infer_chunk(&self, chunk: Vec<f32>) -> Result<String, String> {
        let slot = Arc::clone(&self.stream);
        tauri::async_runtime::spawn_blocking(move || run_chunk_blocking(slot, chunk))
            .await
            .map_err(|e| format!("Parakeet inference task panicked: {e}"))?
    }
}

/// Pre-fills a freshly created stream's internal ring buffer with exactly
/// one second of silence — `parakeet-rs`'s own `MIN_BUFFER_SAMPLES` gate
/// (`SAMPLE_RATE`, 16,000 samples; see the vendored crate's
/// `parakeet_eou.rs`). Without this, every fresh "Transcribe" click starts
/// a brand-new [`ParakeetEOU`] stream with an empty buffer, and the crate
/// silently returns empty text for every chunk until a full second of real
/// audio has arrived and accumulated — real speech spoken in that window
/// isn't lost, it's just invisible on screen for that first second (plus
/// however long the person took to start talking after clicking the
/// button). Root-caused with a real captured session log and this crate's
/// own source; see `docs/evidence.md`'s "STT startup/pause latency
/// investigation" entry. Priming satisfies the gate immediately at load
/// time, using silence Aside generates itself rather than waiting on the
/// microphone, so the very first *real* audio chunk pushed can decode
/// right away instead of waiting behind a second of its own audio.
/// `reset_on_eou: false` — silence should never emit a stop token, but
/// even if it did, `false` avoids resetting decoder state that has not
/// done anything real yet.
fn prime_stream(stream: &mut ParakeetEOU) {
    let silence = vec![0.0_f32; STT_SAMPLE_RATE as usize];
    let _ = stream.transcribe(&silence, false);
}

fn get_or_load_shared(model_dir: &Path) -> Result<SharedHandle, String> {
    let mut slot = shared_slot().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(existing) = slot.upgrade() {
        return Ok(existing);
    }
    let loaded = ParakeetEOUHandle::from_pretrained(model_dir, None).map_err(|e| {
        format!(
            "failed to load Parakeet EOU model at {}: {e}",
            model_dir.display()
        )
    })?;
    let shared = Arc::new(loaded);
    *slot = Arc::downgrade(&shared);
    Ok(shared)
}

fn run_chunk_blocking(slot: Arc<Mutex<ParakeetEOU>>, chunk: Vec<f32>) -> Result<String, String> {
    let mut stream = slot
        .lock()
        .map_err(|_| "Parakeet stream lock poisoned".to_string())?;
    // `reset_on_eou: true` is required for the crate to append `" [EOU]"` —
    // with `false` the token is swallowed and Aside would never see a stop.
    stream
        .transcribe(&chunk, true)
        .map_err(|e| format!("parakeet EOU inference failed: {e}"))
}

/// Splits a decoder delta into text and whether this chunk ended an utterance.
/// The crate appends a literal `" [EOU]"` when `reset_on_eou` is true.
pub fn split_eou(delta: &str) -> (String, bool) {
    for marker in [" [EOU]", "[EOU]"] {
        if let Some(idx) = delta.find(marker) {
            let mut piece = String::new();
            piece.push_str(&delta[..idx]);
            piece.push_str(&delta[idx + marker.len()..]);
            return (piece, true);
        }
    }
    (delta.to_string(), false)
}

/// SentencePiece's word-boundary marker (U+2581, "LOWER ONE EIGHTH BLOCK",
/// `▁`). The downloaded tokenizer's `decoder.replacement` field is
/// misconfigured to `" "` instead of `"▁"` (an external cache artifact, not
/// something Aside can fix by patching a downloaded file), so the
/// `tokenizers` crate's own `decode()` never converts it and it passes
/// straight through into `parakeet-rs`'s per-token output — visible in the
/// UI as stray `▁` glyphs where spaces belong. Converted here, once, so
/// every caller (`drain` and `flush`, both of which go through
/// `apply_delta`) gets clean text without duplicating the fix.
const SENTENCEPIECE_WORD_BOUNDARY: char = '\u{2581}';

/// Folds a decoder delta into the open utterance. Returns a preview when
/// the text grew, or a hallucination-filtered final on EOU.
pub fn apply_delta(utterance: &mut String, delta: &str) -> Option<Transcript> {
    let (piece, eou) = split_eou(delta);
    let mut piece = piece.replace(SENTENCEPIECE_WORD_BOUNDARY, " ");
    if utterance.is_empty() {
        // SentencePiece marks the start of every word, including the very
        // first one in a fresh utterance — converted verbatim that leaves a
        // stray leading space. The bench UI renders transcript text with
        // `white-space: pre-wrap` (`src/styles/bench.css`), which preserves
        // leading spaces instead of collapsing them, so this would show as a
        // visible indent rather than being silently absorbed. Only the very
        // first piece of a new utterance needs this; mid-utterance markers
        // are the real word-separating spaces and must stay.
        piece = piece.trim_start().to_string();
    }
    if !piece.is_empty() {
        utterance.push_str(&piece);
    }
    if eou {
        let text = std::mem::take(utterance);
        return hallucination_filter::clean(&text).map(|text| Transcript {
            text,
            is_final: true,
        });
    }
    if piece.is_empty() || utterance.is_empty() {
        return None;
    }
    Some(Transcript {
        text: utterance.clone(),
        is_final: false,
    })
}

#[async_trait::async_trait]
impl SttEngine for ParakeetLocalEngine {
    fn push_audio(&mut self, samples: &[f32], sample_rate: u32) {
        let resampled = resample_linear(samples, sample_rate, STT_SAMPLE_RATE);
        self.pending.extend_from_slice(&resampled);
    }

    async fn drain(&mut self) -> Result<Vec<Transcript>, String> {
        let mut out = Vec::new();
        while self.pending.len() >= CHUNK_SAMPLES {
            let chunk: Vec<f32> = self.pending.drain(..CHUNK_SAMPLES).collect();
            let delta = self.infer_chunk(chunk).await?;
            if let Some(t) = apply_delta(&mut self.utterance, &delta) {
                out.push(t);
            }
        }
        Ok(out)
    }

    async fn flush(&mut self) -> Result<Vec<Transcript>, String> {
        // Only used when the user stops transcription — not on speaking-light
        // VAD. Pad a short remainder to a full chunk, then feed silence so
        // the decoder can emit a trailing EOU for audio already in the ring.
        if !self.pending.is_empty() {
            let mut last = std::mem::take(&mut self.pending);
            last.resize(CHUNK_SAMPLES, 0.0);
            let delta = self.infer_chunk(last).await?;
            let _ = apply_delta(&mut self.utterance, &delta);
        }
        let mut out = Vec::new();
        for _ in 0..FLUSH_SILENCE_CHUNKS {
            let delta = self.infer_chunk(vec![0.0; CHUNK_SAMPLES]).await?;
            if let Some(t) = apply_delta(&mut self.utterance, &delta) {
                out.push(t);
            }
        }
        if !self.utterance.is_empty() {
            if let Some(text) = hallucination_filter::clean(&std::mem::take(&mut self.utterance)) {
                out.push(Transcript {
                    text,
                    is_final: true,
                });
            }
        }
        Ok(out.into_iter().filter(|t| t.is_final).collect())
    }

    async fn shutdown(self: Box<Self>) -> Vec<Transcript> { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_size_is_exactly_160ms_at_16khz() {
        assert_eq!(CHUNK_SAMPLES, 2560);
        assert_eq!(CHUNK_SAMPLES * 1000 / STT_SAMPLE_RATE as usize, 160);
    }

    #[test]
    fn split_eou_detects_the_crate_marker_and_keeps_surrounding_text() {
        assert_eq!(split_eou("hello"), ("hello".to_string(), false));
        assert_eq!(split_eou("hello [EOU]"), ("hello".to_string(), true));
        assert_eq!(split_eou(" [EOU]"), ("".to_string(), true));
        assert_eq!(split_eou("hi [EOU] there"), ("hi there".to_string(), true));
    }

    #[test]
    fn apply_delta_grows_a_preview_then_commits_the_whole_utterance_on_eou() {
        let mut u = String::new();
        let preview = apply_delta(&mut u, "so tell me").unwrap();
        assert!(!preview.is_final);
        assert_eq!(preview.text, "so tell me");
        let preview = apply_delta(&mut u, " about a time").unwrap();
        assert!(!preview.is_final);
        assert_eq!(preview.text, "so tell me about a time");
        let fin = apply_delta(&mut u, " [EOU]").unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "so tell me about a time");
        assert!(
            u.is_empty(),
            "next utterance must start clean, leftover {u:?}"
        );
    }

    #[test]
    fn apply_delta_drops_a_known_hallucination_at_eou() {
        let mut u = String::new();
        assert!(apply_delta(&mut u, "Thank you for watching [EOU]").is_none());
        assert!(u.is_empty());
    }

    #[test]
    fn apply_delta_converts_sentencepiece_word_boundary_markers_to_spaces() {
        // Reproduces the reported symptom directly: a delta carrying raw
        // `▁` markers (what the misconfigured tokenizer.json decoder
        // actually hands back) must come out as normal spaces, not `▁` or
        // anything underscore-like.
        let mut u = String::new();
        let preview = apply_delta(&mut u, "\u{2581}can\u{2581}you\u{2581}help\u{2581}me").unwrap();
        assert!(!preview.is_final);
        assert_eq!(preview.text, "can you help me");
        assert!(!preview.text.contains('\u{2581}'));
        assert!(!preview.text.contains('_'));
    }

    #[test]
    fn apply_delta_does_not_leave_a_leading_space_from_the_first_word_boundary_marker() {
        // SentencePiece marks the start of the very first word too — that
        // marker must be trimmed, not turned into a leading space, since the
        // bench UI's `white-space: pre-wrap` would render it as a visible
        // indent instead of collapsing it.
        let mut u = String::new();
        let preview = apply_delta(&mut u, "\u{2581}hello").unwrap();
        assert_eq!(preview.text, "hello");
        assert!(!preview.text.starts_with(' '));
    }

    #[test]
    fn apply_delta_keeps_normal_spacing_between_words_across_multiple_deltas() {
        // Only the very first piece of a fresh utterance gets its leading
        // marker trimmed — later deltas' markers are real word separators
        // and must become ordinary spaces, not get eaten too.
        let mut u = String::new();
        apply_delta(&mut u, "\u{2581}so").unwrap();
        let preview = apply_delta(&mut u, "\u{2581}tell\u{2581}me").unwrap();
        assert!(!preview.is_final);
        assert_eq!(preview.text, "so tell me");
    }

    #[test]
    fn apply_delta_commits_a_word_boundary_marker_utterance_cleanly_on_eou() {
        let mut u = String::new();
        apply_delta(&mut u, "\u{2581}can\u{2581}you\u{2581}help\u{2581}me").unwrap();
        let fin = apply_delta(&mut u, " [EOU]").unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "can you help me");
        assert!(
            u.is_empty(),
            "next utterance must start clean, leftover {u:?}"
        );
    }

    #[test]
    fn apply_delta_ignores_empty_chunks_without_eou() {
        let mut u = String::new();
        assert!(apply_delta(&mut u, "").is_none());
        apply_delta(&mut u, "hello").unwrap();
        assert!(apply_delta(&mut u, "").is_none());
        assert_eq!(u, "hello");
    }

    #[test]
    fn an_empty_non_eou_delta_mid_utterance_does_not_get_treated_as_a_fresh_start() {
        // The specific risk adversarial review traced through: the
        // `utterance.is_empty()` check exists so the *very first* piece of
        // a genuinely new utterance gets its leading SentencePiece marker
        // trimmed. If an empty (no new tokens this chunk, no EOU) delta
        // arrived mid-utterance and were mistaken for "utterance was just
        // reset, so this is a fresh start," the *next* real piece's leading
        // marker-turned-space would wrongly get trimmed instead of staying
        // a normal word separator — silently merging two words. It
        // doesn't: an empty delta never touches `utterance` (nothing is
        // pushed when `piece` is empty), so `utterance.is_empty()` stays
        // false the whole time this word is in progress, and the trim-only-
        // when-empty rule only ever fires at a real EOU-reset boundary.
        let mut u = String::new();
        apply_delta(&mut u, "\u{2581}Hello").unwrap();
        // A chunk where the model emitted no new tokens at all — plausible
        // mid-word, e.g. still buffering audio for the next piece.
        assert!(apply_delta(&mut u, "").is_none());
        assert_eq!(
            u, "Hello",
            "an empty delta must not alter the utterance in progress"
        );
        let preview = apply_delta(&mut u, "\u{2581}world").unwrap();
        assert_eq!(
            preview.text, "Hello world",
            "the word-boundary marker on the real next piece must still become a normal \
             separating space, not get trimmed as if this were a fresh utterance"
        );
        let fin = apply_delta(&mut u, " [EOU]").unwrap();
        assert_eq!(fin.text, "Hello world");
        assert!(u.is_empty());
    }

    /// **Measures the real "click Transcribe → audio route is live" window**
    /// — a candidate root cause for the "sometimes the first few words are
    /// gone" report, distinct from the already-fixed `MIN_BUFFER_SAMPLES`
    /// gate. `commands/stt.rs::start_channel_transcription` only installs
    /// `TranscriptionRouting`'s sender *after* `build_engine().await`
    /// resolves (`pipeline.rs::start`, `route_for(...).lock() = Some(tx)`);
    /// `audio/channel.rs::forward_to_stt` only sends when that sender is
    /// already present — there is no pre-route buffer, so real mic/system
    /// audio captured before the route exists is not delayed, it is
    /// permanently discarded. Run singly (not with other tests in the same
    /// process) so the first measurement is genuinely cold:
    /// `cargo test --release --lib stt::parakeet_local::tests::measures_the_real_click_to_route_live_window -- --ignored --nocapture --test-threads=1`
    #[tokio::test]
    #[ignore]
    async fn measures_the_real_click_to_route_live_window() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);

        let cold_start = std::time::Instant::now();
        let first = ParakeetLocalEngine::load(&path).expect("cold load should succeed");
        let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;

        // Second channel starting Transcribe in the same app run — this is
        // the far more common case in real use (one channel is usually
        // already running by the time the other is toggled on).
        let warm_start = std::time::Instant::now();
        let second = ParakeetLocalEngine::load(&path).expect("warm load should succeed");
        let warm_ms = warm_start.elapsed().as_secs_f64() * 1000.0;

        eprintln!("=== click-Transcribe-to-route-live window ===");
        eprintln!("cold (first engine this process, real ONNX weight load + session init + priming): {cold_ms:.1} ms");
        eprintln!(
            "warm (shared weights already loaded, only a fresh stream + priming): {warm_ms:.1} ms"
        );
        eprintln!(
            "=> a person who starts talking the instant they click \"Transcribe\" loses up to \
             ~{cold_ms:.0}ms of real audio on the very first engine of a session (nowhere to go — \
             no route exists yet), or ~{warm_ms:.0}ms on every later start/stop of a channel."
        );

        drop(first);
        drop(second);
    }

    #[test]
    #[ignore]
    fn two_loads_against_the_same_files_share_one_arc() {
        let models_root = dirs_app_models();
        if !super::super::parakeet_cache::is_downloaded(&models_root) {
            return;
        }
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let a = ParakeetLocalEngine::load(&path).expect("first load");
        let b = ParakeetLocalEngine::load(&path).expect("second load");
        assert!(
            Arc::ptr_eq(&a.handle, &b.handle),
            "mic and system must share one loaded Parakeet EOU handle, not two copies"
        );
    }

    /// Real model, real inference. Deliberately `#[ignore]`d so ordinary
    /// `cargo test` stays fast and offline-safe. Run with
    /// `cargo test --lib stt::parakeet_local -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn installed_model_loads_and_transcribes_silence_as_silence() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let mut engine =
            ParakeetLocalEngine::load(&path).expect("installed Parakeet EOU model should load");
        engine.push_audio(&[0.0_f32; 16_000], 16_000);
        let drained = engine
            .drain()
            .await
            .expect("silence drain should not error");
        let flushed = engine
            .flush()
            .await
            .expect("silence flush should not error");
        for t in drained.iter().chain(flushed.iter()) {
            assert!(
                t.text.trim().is_empty(),
                "silence should not produce a hallucinated transcript, got {t:?}"
            );
        }
        Box::new(engine).shutdown().await;
    }

    fn dirs_app_models() -> std::path::PathBuf {
        let home = std::env::var("HOME").expect("HOME");
        std::path::PathBuf::from(home).join("Library/Application Support/com.aside.desktop/models")
    }

    /// A deterministic, non-silent, spectrally-varied 160ms chunk — a few
    /// sine components in speech's typical F0/formant range with a
    /// wandering pitch, not pure silence. Silence tends to make the
    /// decoder's `while syms_added < 5` loop (`parakeet_eou.rs`) exit on
    /// the very first blank logit every frame — cheap, and not what real
    /// speech does (real speech keeps that loop emitting real tokens up to
    /// its 5-symbol cap most frames). Using synthetic tone content instead
    /// of real recorded speech avoids shipping/depending on an audio
    /// fixture and avoids a new `rand` dependency for something this
    /// simple, while still exercising the decoder's real per-frame work
    /// instead of its fastest possible path.
    fn synthetic_speech_like_chunk(chunk_index: usize) -> Vec<f32> {
        let sr = STT_SAMPLE_RATE as f32;
        let base_hz = 120.0 + (chunk_index % 5) as f32 * 15.0; // wanders 120-180Hz
        (0..CHUNK_SAMPLES)
            .map(|n| {
                let t = n as f32 / sr;
                let f0 = (2.0 * std::f32::consts::PI * base_hz * t).sin();
                let f1 = 0.5 * (2.0 * std::f32::consts::PI * base_hz * 2.7 * t).sin();
                let f2 = 0.25 * (2.0 * std::f32::consts::PI * base_hz * 4.3 * t).sin();
                0.2 * (f0 + f1 + f2)
            })
            .collect()
    }

    /// **Step 1 of the pause/lag investigation.** Times each individual
    /// 160ms-chunk `infer_chunk` call against real wall-clock time, using
    /// the actual installed model, to answer directly: is one 160ms chunk
    /// slower to process than 160ms of real time (real-time factor > 1.0)?
    /// If so, live speech can never be kept up with and a backlog must grow
    /// without bound for the rest of the session. Run with
    /// `cargo test --lib stt::parakeet_local -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn measures_real_per_chunk_inference_latency_and_real_time_factor() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let engine =
            ParakeetLocalEngine::load(&path).expect("installed Parakeet EOU model should load");

        // 50 chunks = 8.0s of audio. `load()` now primes the stream's ring
        // buffer with 1s of silence (`prime_stream`, fixing the "slow to
        // appear when a speaker starts talking" bug — see
        // `docs/evidence.md`'s STT latency investigation entry), so the
        // crate's MIN_BUFFER_SAMPLES gate is already satisfied before this
        // loop's first chunk — every chunk below does real encoder/decoder
        // work, not just chunk 7 onward like before that fix. `WARMUP_CHUNKS`
        // here only excludes the very first chunk, which still costs a few
        // extra ms of first-call ONNX Runtime overhead (thread pool /
        // allocator spin-up), from the steady-state average.
        const NUM_CHUNKS: usize = 50;
        const WARMUP_CHUNKS: usize = 1;
        let mut chunk_ms: Vec<f64> = Vec::with_capacity(NUM_CHUNKS);

        for i in 0..NUM_CHUNKS {
            let chunk = synthetic_speech_like_chunk(i);
            let start = std::time::Instant::now();
            let _ = engine
                .infer_chunk(chunk)
                .await
                .expect("inference should not error on synthetic audio");
            chunk_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let warm = &chunk_ms[WARMUP_CHUNKS..];
        let sum: f64 = warm.iter().sum();
        let avg = sum / warm.len() as f64;
        let max = warm.iter().cloned().fold(f64::MIN, f64::max);
        let min = warm.iter().cloned().fold(f64::MAX, f64::min);
        let rtf = avg / 160.0;

        eprintln!("=== Parakeet EOU per-chunk inference timing (synthetic speech-like audio, CPU, default intra_threads) ===");
        for (i, ms) in chunk_ms.iter().enumerate() {
            eprintln!(
                "  chunk {i:>3}: {ms:>9.2} ms{}",
                if i < WARMUP_CHUNKS {
                    "  (first-call ONNX Runtime overhead)"
                } else {
                    ""
                }
            );
        }
        eprintln!(
            "steady-state (chunks {WARMUP_CHUNKS}..{NUM_CHUNKS}): avg={avg:.2}ms min={min:.2}ms max={max:.2}ms over {} chunks",
            warm.len()
        );
        eprintln!("real-time factor (avg_ms / 160ms of audio): {rtf:.3}");
        eprintln!(
            "=> {}",
            if rtf < 1.0 {
                "under 1.0: a single chunk processes faster than real time on this hardware"
            } else {
                "1.0 or over: a single chunk is slower than the audio it represents — backlog WILL grow without bound"
            }
        );

        // Not a hardware-speed pass/fail gate — this test exists to print
        // real numbers (the empirical-measurement requirement). Only guard
        // against a silently-broken measurement (e.g. a mocked/instant path).
        assert!(
            avg > 0.0,
            "measured zero-cost inference — timing is broken, not fast"
        );
    }

    /// **Root-cause regression test for the "slow to appear when a speaker
    /// starts talking" bug.** Before the fix, `ParakeetLocalEngine::load()`
    /// handed back a stream with an empty ring buffer, so the crate's own
    /// `MIN_BUFFER_SAMPLES` gate (1s of audio, `parakeet_eou.rs`) made every
    /// chunk for the first ~1s of real speech after a fresh "Transcribe"
    /// click return instantly with no real work and no text — confirmed on
    /// this machine (`measures_real_per_chunk_inference_latency_and_real_time_factor`,
    /// pre-fix run: chunks 0-5 each took well under 1ms). `load()` now
    /// primes the stream with 1s of silence (`prime_stream`) before handing
    /// it back, so the gate is already satisfied — the very first *real*
    /// chunk pushed after `load()` must do real encoder/decoder work
    /// immediately, not return an instant empty string. If `prime_stream`
    /// is ever removed or stops running, this assertion fails because the
    /// first chunk goes back to costing under a millisecond.
    #[tokio::test]
    #[ignore]
    async fn load_primes_the_buffer_so_the_first_real_chunk_runs_real_inference() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let engine =
            ParakeetLocalEngine::load(&path).expect("installed Parakeet EOU model should load");

        let start = std::time::Instant::now();
        let _ = engine
            .infer_chunk(synthetic_speech_like_chunk(0))
            .await
            .expect("inference should not error on synthetic audio");
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        eprintln!("first real chunk after load(): {elapsed_ms:.2} ms");
        assert!(
            elapsed_ms >= 5.0,
            "first chunk after load() returned in {elapsed_ms:.2}ms — that's the pre-fix \
             'buffer not full yet, instant empty return' path, not real inference. \
             prime_stream() in load() may have been removed or broken."
        );
    }

    /// Minimal 16-bit PCM mono WAV reader for test fixtures only — finds the
    /// `data` chunk by walking RIFF sub-chunks (not a fixed byte offset,
    /// since `afconvert`'s output includes a `fmt ` chunk of unspecified
    /// size ahead of it) and converts samples to the `f32` range this
    /// engine's `infer_chunk` expects.
    fn read_wav_pcm16_mono(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("test fixture WAV should be readable");
        assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file: {path:?}");
        assert_eq!(&bytes[8..12], b"WAVE", "not a WAVE file: {path:?}");
        let mut pos = 12;
        while pos + 8 <= bytes.len() {
            let chunk_id = &bytes[pos..pos + 4];
            let chunk_size =
                u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let data_start = pos + 8;
            if chunk_id == b"data" {
                let data = &bytes[data_start..data_start + chunk_size];
                return data
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
            }
            pos = data_start + chunk_size + (chunk_size % 2); // chunks are word-aligned
        }
        panic!("no data chunk found in {path:?}");
    }

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/stt")
            .join(name)
    }

    /// **Measurement pass for the "words lost right after a pause/EOU"
    /// report** — using real synthesized speech (`say`, converted to 16kHz
    /// mono PCM via `afconvert`), not synthetic tones, since the model
    /// never emits any real tokens for pure sine-wave "speech-like" audio
    /// (confirmed empirically: it returned `""` for every chunk of
    /// [`synthetic_speech_like_chunk`] in an earlier run of this
    /// investigation — real acoustic content is required to exercise the
    /// decoder's actual word-recognition path, not just its timing).
    ///
    /// `reset_states()` in the vendored crate (`parakeet_eou.rs`) fires on
    /// every `[EOU]`, not just at stream creation — it zeroes
    /// `state_h`/`state_c`/`last_token` while deliberately leaving
    /// `encoder_cache`/`audio_buffer` running ("keep ... flowing for
    /// continuous context", the crate's own comment). This feeds one real
    /// utterance, forces a real EOU with trailing silence (the same
    /// mechanism `flush()` already relies on in production), then
    /// immediately — no extra silence — feeds a second, different real
    /// utterance and checks whether its opening words appear in the
    /// resulting text. Run with
    /// `cargo test --lib stt::parakeet_local -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_speech_immediately_after_an_eou_reset_is_not_silently_lost() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let engine =
            ParakeetLocalEngine::load(&path).expect("installed Parakeet EOU model should load");

        // "So tell me about a time you solved a difficult problem at work"
        let utt_a = read_wav_pcm16_mono(&fixture_path("utt_a.wav"));
        // "I once had to debug a production outage under a lot of pressure"
        let utt_b = read_wav_pcm16_mono(&fixture_path("utt_b.wav"));

        let mut utterance_a = String::new();
        for chunk in utt_a.chunks(CHUNK_SAMPLES) {
            let mut padded = chunk.to_vec();
            padded.resize(CHUNK_SAMPLES, 0.0);
            let delta = engine
                .infer_chunk(padded)
                .await
                .expect("utt_a inference should not error");
            apply_delta(&mut utterance_a, &delta);
        }
        eprintln!("recognized from utt_a (before EOU): {utterance_a:?}");

        // Force a real EOU with trailing silence, exactly like `flush()`.
        let mut eou_seen = false;
        for _ in 0..15 {
            let delta = engine
                .infer_chunk(vec![0.0; CHUNK_SAMPLES])
                .await
                .expect("silence inference should not error");
            if apply_delta(&mut utterance_a, &delta)
                .map(|t| t.is_final)
                .unwrap_or(false)
            {
                eou_seen = true;
                break;
            }
        }
        assert!(
            eou_seen,
            "test setup failed to make the real model emit [EOU] from trailing silence — cannot measure recovery without a real reset"
        );
        eprintln!("utt_a final (post-EOU): {utterance_a:?}");

        // Immediately resume with a second, different real utterance — no
        // extra silence — the exact shape of "kept talking right after a
        // pause," which the earlier investigation found is not specially
        // handled anywhere in this pipeline.
        let mut utterance_b = String::new();
        for chunk in utt_b.chunks(CHUNK_SAMPLES) {
            let mut padded = chunk.to_vec();
            padded.resize(CHUNK_SAMPLES, 0.0);
            let delta = engine
                .infer_chunk(padded)
                .await
                .expect("utt_b inference should not error");
            apply_delta(&mut utterance_b, &delta);
        }
        eprintln!("recognized from utt_b (immediately after EOU, no gap): {utterance_b:?}");

        // Not a hard pass/fail gate on exact wording (real ASR output on
        // synthesized speech is naturally imperfect) — this is the
        // measurement pass. The opening word "I" is trivially easy for any
        // working ASR to catch; its absence (or a several-word gap before
        // recognizable content starts) is the direct, observable signature
        // of the reported bug.
        eprintln!(
            "=> {}",
            if utterance_b.trim().is_empty() {
                "NOTHING was recognized from the second utterance at all — total loss"
            } else if !utterance_b.to_lowercase().contains("debug")
                && !utterance_b.to_lowercase().starts_with('i')
            {
                "recognized text is missing the utterance's opening words — partial loss, matches the report"
            } else {
                "recognized text captured the opening of the second utterance"
            }
        );
    }

    /// **Step 1, backlog scenario.** Simulates a long pause: audio keeps
    /// arriving (mic capture never stops) but nothing gets drained until a
    /// large amount has queued up — feeds `push_audio` with far more than
    /// one chunk's worth before ever calling `drain()`, then times how long
    /// the single resulting `drain()` call takes to work through the entire
    /// backlog it finds queued in `pending`. This is the direct real-world
    /// analogue of "pause for N seconds, then speak" — answers whether
    /// catch-up lag is bounded or grows with the length of the pause.
    #[tokio::test]
    #[ignore]
    async fn measures_catchup_lag_after_a_simulated_long_pause_backlog() {
        let models_root = dirs_app_models();
        assert!(
            super::super::parakeet_cache::is_downloaded(&models_root),
            "Parakeet Realtime EOU 120M is not downloaded at {} — use the Bench download button first",
            super::super::parakeet_cache::model_dir(&models_root).display()
        );
        let path = super::super::parakeet_cache::model_dir(&models_root);
        let mut engine =
            ParakeetLocalEngine::load(&path).expect("installed Parakeet EOU model should load");

        // Prime past the crate's 1s warm-up gate first, draining it off so
        // the backlog measurement below isolates catch-up cost, not
        // warm-up cost (that's the separate, already-documented ~1s gate).
        for i in 0..7 {
            engine.push_audio(&synthetic_speech_like_chunk(i), 16_000);
        }
        let _ = engine
            .drain()
            .await
            .expect("priming drain should not error");

        // 15 seconds of audio queued up without a single intervening
        // drain() — the direct analogue of a 15s pause where capture kept
        // running (VAD never gates STT in this app) but nothing was
        // consumed. `pending` is currently unbounded, so all of it queues.
        const PAUSE_SECONDS: f64 = 15.0;
        let backlog_samples = (PAUSE_SECONDS * STT_SAMPLE_RATE as f64) as usize;
        let mut pushed = 0usize;
        let mut i = 100; // offset so this doesn't reuse the priming chunks' exact content
        while pushed < backlog_samples {
            let chunk = synthetic_speech_like_chunk(i);
            pushed += chunk.len();
            engine.push_audio(&chunk, 16_000);
            i += 1;
        }
        assert!(
            engine.pending.len() >= CHUNK_SAMPLES,
            "test setup didn't actually queue a backlog"
        );

        let start = std::time::Instant::now();
        let out = engine
            .drain()
            .await
            .expect("drain should not error working through a backlog");
        let elapsed = start.elapsed();

        eprintln!("=== Backlog catch-up: {PAUSE_SECONDS}s of audio queued during a simulated pause, single drain() call ===");
        eprintln!(
            "wall-clock time to process the entire backlog: {:.2} ms ({:.2} s)",
            elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64()
        );
        eprintln!(
            "catch-up ratio (processing time / paused audio duration): {:.3}",
            elapsed.as_secs_f64() / PAUSE_SECONDS
        );
        eprintln!(
            "transcript segments emitted from the backlog: {}",
            out.len()
        );
        eprintln!(
            "=> {}",
            if elapsed.as_secs_f64() < PAUSE_SECONDS {
                "backlog was cleared faster than the pause was long — a single long pause alone would not permanently fall behind"
            } else {
                "backlog took LONGER to process than the pause itself lasted — this actively falls further behind, not just slow to start"
            }
        );
    }
}
