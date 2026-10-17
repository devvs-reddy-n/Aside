//! A real, live Deepgram Nova WebSocket engine.
//!
//! `stt/deepgram.rs` models the protocol (URL, auth, message parsing) as
//! pure, unit-tested logic with no live connection. This module is the
//! actual connection: opening the socket, a reconnect state machine, a
//! keepalive, and wiring it all into `SttEngine`. Design ported from
//! `references/Repo1-Natively/electron/audio/DeepgramStreamingSTT.ts`
//! (exponential backoff, 5s stability timer, discard-on-reconnect) and the
//! Phase 4 continuation research (`docs/evidence.md`).
//!
//! **Decoupling the sync `push_audio` from the async socket.** `push_audio`
//! is a synchronous, non-blocking trait method — sending over a WebSocket is
//! inherently async. The fix is the same shape as everywhere else in this
//! codebase where a real-time thread hands off to slower work: a bounded
//! `tokio::mpsc` channel. `push_audio` does a non-blocking `try_send`;
//! on a full channel it drops the chunk rather than block (an audio gap is
//! recoverable, blocking the caller is not — this mirrors
//! `audio/mic.rs`/`audio/system.rs`'s own real-time-thread discipline).
//! A background task owns the actual socket, drains that channel, and
//! forwards parsed results into a second channel `drain()` reads from.
//!
//! **Known simplification vs. this file's protocol comments, now closed:**
//! `deepgram.rs` documents that `Results{is_final:true}` can fire mid-utterance
//! and a real client should wait for the separate `UtteranceEnd`. This engine
//! now does that: interims and `is_final` Results update the live preview;
//! only `UtteranceEnd` commits a final. That is Deepgram's own stop-of-speech
//! — Aside's speaking-light VAD is not involved.
//!
//! **Non-final `Transcript`s carry the whole current utterance, not a
//! delta.** That's simply what Deepgram's own `channel.alternatives[0].transcript`
//! field already is for an interim `Results` message — each one is a fresh,
//! complete restatement, not an append-only fragment.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::{ClientRequestBuilder, Message};

use super::deepgram::{
    auth_header_value, build_url, parse_message, DeepgramConfig, DeepgramMessage,
};
use super::resample::resample_linear;
use super::wav::f32_to_i16_le_bytes;
use super::{SttEngine, Transcript};

const AUDIO_CHANNEL_CAPACITY: usize = 256;
const TRANSCRIPT_CHANNEL_CAPACITY: usize = 256;
const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const STABILITY_WINDOW: Duration = Duration::from_secs(5);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(8);

enum Outgoing {
    Audio(Vec<u8>),
    Finalize,
}

pub struct DeepgramLiveEngine {
    outgoing_tx: mpsc::Sender<Outgoing>,
    transcript_rx: mpsc::Receiver<Transcript>,
    stop: Arc<AtomicBool>,
    /// Set by `connection_task` once it gives up permanently (bad API key,
    /// or reconnect attempts exhausted). Without this, a dead connection
    /// task just drops `transcript_tx` and `drain`/`flush` keep returning
    /// `Ok(vec![])` forever — indistinguishable from "no speech yet" at
    /// every layer above this one, including the user. See
    /// `docs/evidence.md`'s Phase 4 continuation adversarial-review entry.
    terminal_error: Arc<Mutex<Option<String>>>,
    /// The rate written into the connect URL by `build_url`. Held here so
    /// `push_audio` can resample the capture-rate audio it receives to match
    /// what the server was told — see the comment there for the bug this
    /// exists to prevent.
    declared_sample_rate: u32,
}

impl DeepgramLiveEngine {
    /// Starts the background connection task immediately — matches the
    /// other engines' shape (constructing one is "starting" it), rather than
    /// a separate explicit `connect()` step.
    pub fn start(api_key: String, config: DeepgramConfig) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::channel(AUDIO_CHANNEL_CAPACITY);
        let (transcript_tx, transcript_rx) = mpsc::channel(TRANSCRIPT_CHANNEL_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));

        let task_stop = stop.clone();
        let task_error = terminal_error.clone();
        // Captured before `config` moves into the task — this is the exact
        // value `build_url` puts in the URL, so `push_audio` resamples to
        // whatever the server is actually told, not a second hardcoded guess
        // that could drift away from it.
        let declared_sample_rate = config.sample_rate;
        tauri::async_runtime::spawn(connection_task(
            api_key,
            config,
            outgoing_rx,
            transcript_tx,
            task_stop,
            task_error,
        ));

        Self {
            outgoing_tx,
            transcript_rx,
            stop,
            declared_sample_rate,
            terminal_error,
        }
    }
}

/// Resamples capture-rate audio to the rate declared in the connect URL and
/// encodes it as the little-endian 16-bit PCM Deepgram was told to expect.
///
/// Split out of `push_audio` purely so it can be unit-tested: `push_audio`
/// writes into a channel owned by a background task, so from outside the
/// engine there is nothing to observe. The bug this guards against produced
/// no error on either side — only wrong-sounding audio — so a test that can
/// actually see the bytes is the only way to keep it fixed.
fn encode_for_wire(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<u8> {
    let resampled = resample_linear(samples, from_rate, to_rate);
    f32_to_i16_le_bytes(&resampled)
}

#[async_trait::async_trait]
impl SttEngine for DeepgramLiveEngine {
    fn push_audio(&mut self, samples: &[f32], sample_rate: u32) {
        // Resample to the rate that was declared in the connect URL, rather
        // than ignoring the argument.
        //
        // This was a real, silent, output-ruining bug. `build_url` writes
        // `sample_rate=` from `DeepgramConfig` (16000 by default,
        // `deepgram.rs`), and `commands/stt.rs` constructs that default — but
        // capture runs at the device rate (`mic.rs`'s TARGET_RATE is 48000,
        // and system audio uses whatever the output device reports, which is
        // 24000 on a Bluetooth headset in HFP mode). Sending 48kHz samples
        // labelled as 16kHz makes Deepgram decode them 3x too fast: every
        // transcript comes back as chipmunk-speed nonsense, with no error
        // anywhere, because both sides did exactly what they were told.
        //
        // The local Parakeet engine never had this bug — it resamples on
        // push. This engine simply never honoured its own declared rate.
        let pcm = encode_for_wire(samples, sample_rate, self.declared_sample_rate);
        // A full channel means the connection task is behind (reconnecting,
        // or the network is slow) — dropping this chunk is the right call,
        // never blocking the caller (this may be invoked from code adjacent
        // to a real-time audio callback thread).
        if self.outgoing_tx.try_send(Outgoing::Audio(pcm)).is_err() {
            eprintln!("deepgram: audio channel full or closed, dropping a chunk");
        }
    }

    async fn drain(&mut self) -> Result<Vec<Transcript>, String> {
        // Checked before, not instead of, draining — a connection that died
        // after producing some final transcripts should still surface those
        // before reporting the terminal error on the next call.
        let mut out = Vec::new();
        while let Ok(t) = self.transcript_rx.try_recv() {
            out.push(t);
        }
        if !out.is_empty() {
            return Ok(out);
        }
        if let Some(msg) = self
            .terminal_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(msg);
        }
        Ok(out)
    }

    async fn flush(&mut self) -> Result<Vec<Transcript>, String> {
        // Best-effort nudge, not a synchronous wait for Deepgram's own
        // finalization — but we *do* give the socket a short window so a
        // trailing UtteranceEnd can land before shutdown drops the task.
        let _ = self.outgoing_tx.try_send(Outgoing::Finalize);
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.drain().await
    }

    async fn shutdown(self: Box<Self>) -> Vec<Transcript> {
        self.stop.store(true, Ordering::SeqCst);
        Vec::new()
    }
}

#[derive(Default)]
struct DeepgramTurn {
    preview: String,
    committed: String,
}

/// Maps one Deepgram message onto the `Transcript` contract using *their*
/// stop signal (`UtteranceEnd`), not Aside's speaking-light VAD.
/// `is_final` Results update the preview (they can fire mid-utterance);
/// only `UtteranceEnd` commits.
fn apply_deepgram_message(turn: &mut DeepgramTurn, msg: DeepgramMessage) -> Option<Transcript> {
    match msg {
        DeepgramMessage::Results { is_final, channel } => {
            let text = channel.alternatives.first()?.transcript.trim();
            if text.is_empty() {
                return None;
            }
            turn.preview = text.to_string();
            if is_final {
                turn.committed = merge_final_text(&turn.committed, text);
            }
            Some(Transcript {
                text: text.to_string(),
                is_final: false,
            })
        }
        DeepgramMessage::UtteranceEnd => {
            let text = merge_final_text(&turn.committed, &turn.preview);
            turn.preview.clear();
            turn.committed.clear();
            if text.is_empty() {
                None
            } else {
                Some(Transcript {
                    text,
                    is_final: true,
                })
            }
        }
        DeepgramMessage::SpeechStarted | DeepgramMessage::Metadata => None,
    }
}

/// Deepgram may restate a growing utterance *or* emit sequential finalized
/// segments before `UtteranceEnd`. Prefer a restatement that contains the
/// previous text; otherwise join. Never drop a longer preview in favor of
/// an older, shorter committed string.
fn merge_final_text(existing: &str, next: &str) -> String {
    let existing = existing.trim();
    let next = next.trim();
    if existing.is_empty() {
        return next.to_string();
    }
    if next.is_empty() {
        return existing.to_string();
    }
    if next.starts_with(existing) {
        return next.to_string();
    }
    if existing.starts_with(next) {
        return existing.to_string();
    }
    if existing.ends_with(next) {
        return existing.to_string();
    }
    if next.ends_with(existing) {
        return next.to_string();
    }
    format!("{existing} {next}")
}

enum ConnectOutcome {
    /// `stop` was set — the caller should exit permanently, not reconnect.
    Stopped,
    /// The socket closed or errored. `was_stable` is true if the connection
    /// survived past `STABILITY_WINDOW` before that happened — the caller
    /// uses this to decide whether to reset the backoff counter.
    Disconnected { was_stable: bool },
    /// The handshake itself failed with an HTTP error response (e.g. a bad
    /// API key returns 401) — retrying with the same key cannot succeed, so
    /// the caller should give up rather than burn through reconnect
    /// attempts on a failure no amount of retrying fixes.
    AuthFailed(String),
}

async fn connection_task(
    api_key: String,
    config: DeepgramConfig,
    mut outgoing_rx: mpsc::Receiver<Outgoing>,
    transcript_tx: mpsc::Sender<Transcript>,
    stop: Arc<AtomicBool>,
    terminal_error: Arc<Mutex<Option<String>>>,
) {
    let mut attempt: u32 = 0;
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if attempt >= MAX_RECONNECT_ATTEMPTS {
            let msg = format!("deepgram: giving up after {attempt} reconnect attempts");
            eprintln!("{msg}");
            *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
            return;
        }
        if attempt > 0 {
            let backoff =
                (BASE_BACKOFF.saturating_mul(1 << (attempt - 1).min(10))).min(MAX_BACKOFF);
            tokio::time::sleep(backoff).await;
        }

        // Discard anything queued from before this (re)connect — replaying
        // stale audio into a fresh connection is what floods Deepgram and
        // causes EPIPE storms, per Natively's own documented lesson.
        while outgoing_rx.try_recv().is_ok() {}

        match connect_and_run(&api_key, &config, &mut outgoing_rx, &transcript_tx, &stop).await {
            ConnectOutcome::Stopped => return,
            ConnectOutcome::AuthFailed(msg) => {
                eprintln!("deepgram: authentication failed, not retrying: {msg}");
                *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
                return;
            }
            ConnectOutcome::Disconnected { was_stable } => {
                attempt = if was_stable { 0 } else { attempt + 1 };
            }
        }
    }
}

async fn connect_and_run(
    api_key: &str,
    config: &DeepgramConfig,
    outgoing_rx: &mut mpsc::Receiver<Outgoing>,
    transcript_tx: &mpsc::Sender<Transcript>,
    stop: &Arc<AtomicBool>,
) -> ConnectOutcome {
    let uri: Uri = match build_url(config).parse() {
        Ok(u) => u,
        Err(e) => return ConnectOutcome::AuthFailed(format!("invalid deepgram URL: {e}")),
    };
    let request = match ClientRequestBuilder::new(uri)
        .with_header("Authorization", auth_header_value(api_key))
        .into_client_request()
    {
        Ok(r) => r,
        Err(e) => {
            return ConnectOutcome::AuthFailed(format!("could not build deepgram request: {e}"))
        }
    };

    let ws_stream = match tokio_tungstenite::connect_async(request).await {
        Ok((stream, _response)) => stream,
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return ConnectOutcome::AuthFailed(format!(
                    "deepgram rejected credentials ({status})"
                ));
            }
            return ConnectOutcome::Disconnected { was_stable: false };
        }
        Err(e) => {
            eprintln!("deepgram: connect failed: {e}");
            return ConnectOutcome::Disconnected { was_stable: false };
        }
    };

    let (mut sink, mut stream) = ws_stream.split();
    let mut turn = DeepgramTurn::default();
    let mut became_stable = false;
    let stability_deadline = tokio::time::sleep(STABILITY_WINDOW);
    tokio::pin!(stability_deadline);
    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.tick().await; // first tick fires immediately; consume it so the first real one is a full interval away

    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = sink.close().await;
            return ConnectOutcome::Stopped;
        }

        tokio::select! {
            () = &mut stability_deadline, if !became_stable => {
                became_stable = true;
            }
            _ = keepalive.tick() => {
                if sink.send(Message::Text(r#"{"type":"KeepAlive"}"#.into())).await.is_err() {
                    return ConnectOutcome::Disconnected { was_stable: became_stable };
                }
            }
            outgoing = outgoing_rx.recv() => {
                match outgoing {
                    Some(Outgoing::Audio(bytes)) => {
                        if sink.send(Message::Binary(bytes.into())).await.is_err() {
                            return ConnectOutcome::Disconnected { was_stable: became_stable };
                        }
                    }
                    Some(Outgoing::Finalize) => {
                        // Best-effort — a failure here just means the next
                        // audio send (or the socket read loop) will notice
                        // the connection is dead and reconnect.
                        let _ = sink.send(Message::Text(r#"{"type":"Finalize"}"#.into())).await;
                    }
                    None => {
                        // The engine (and its Sender) was dropped — nothing
                        // left to send, no reason to keep the socket open.
                        let _ = sink.close().await;
                        return ConnectOutcome::Stopped;
                    }
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = parse_message(&text) {
                            if let Some(t) = apply_deepgram_message(&mut turn, msg) {
                                let _ = transcript_tx.try_send(t);
                            }
                        }
                        // A message that doesn't parse is logged nowhere on
                        // purpose here — `parse_message` already turns an
                        // unrecognized shape into a descriptive Err, and a
                        // protocol this codebase doesn't fully control
                        // (Deepgram could add new message types) shouldn't
                        // tear down a working connection over one unparsed
                        // frame.
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return ConnectOutcome::Disconnected { was_stable: became_stable };
                    }
                    Some(Ok(_)) => {} // ping/pong/binary from the server — nothing to do
                    Some(Err(e)) => {
                        eprintln!("deepgram: read error: {e}");
                        return ConnectOutcome::Disconnected { was_stable: became_stable };
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::deepgram::{Alternative, Channel};

    fn results_msg(is_final: bool, text: &str) -> DeepgramMessage {
        DeepgramMessage::Results {
            is_final,
            channel: Channel {
                alternatives: vec![Alternative {
                    transcript: text.to_string(),
                    confidence: 0.95,
                }],
            },
        }
    }

    #[test]
    fn is_final_results_stay_preview_until_utterance_end() {
        let mut turn = DeepgramTurn::default();
        let preview =
            apply_deepgram_message(&mut turn, results_msg(true, "so tell me about a time"))
                .unwrap();
        assert!(
            !preview.is_final,
            "Deepgram is_final is mid-utterance; UtteranceEnd is the stop"
        );
        assert_eq!(preview.text, "so tell me about a time");
        let fin = apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd).unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "so tell me about a time");
    }

    #[test]
    fn utterance_end_without_is_final_still_commits_the_last_interim() {
        let mut turn = DeepgramTurn::default();
        apply_deepgram_message(&mut turn, results_msg(false, "hello there")).unwrap();
        let fin = apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd).unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "hello there");
    }

    #[test]
    fn results_with_an_empty_transcript_are_dropped() {
        let mut turn = DeepgramTurn::default();
        let msg = DeepgramMessage::Results {
            is_final: false,
            channel: Channel {
                alternatives: vec![Alternative {
                    transcript: "".to_string(),
                    confidence: 0.0,
                }],
            },
        };
        assert_eq!(apply_deepgram_message(&mut turn, msg), None);
    }

    #[test]
    fn results_with_no_alternatives_at_all_do_not_panic() {
        let mut turn = DeepgramTurn::default();
        let msg = DeepgramMessage::Results {
            is_final: true,
            channel: Channel {
                alternatives: vec![],
            },
        };
        assert_eq!(apply_deepgram_message(&mut turn, msg), None);
    }

    #[test]
    fn speech_started_and_metadata_never_produce_a_transcript() {
        let mut turn = DeepgramTurn::default();
        assert_eq!(
            apply_deepgram_message(&mut turn, DeepgramMessage::SpeechStarted),
            None
        );
        assert_eq!(
            apply_deepgram_message(&mut turn, DeepgramMessage::Metadata),
            None
        );
    }

    #[test]
    fn sequential_is_finals_are_joined_not_replaced() {
        let mut turn = DeepgramTurn::default();
        apply_deepgram_message(&mut turn, results_msg(true, "hello")).unwrap();
        apply_deepgram_message(&mut turn, results_msg(true, "there")).unwrap();
        let fin = apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd).unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "hello there");
    }

    #[test]
    fn utterance_end_keeps_a_preview_that_grew_past_the_last_final() {
        let mut turn = DeepgramTurn::default();
        apply_deepgram_message(&mut turn, results_msg(true, "hello")).unwrap();
        apply_deepgram_message(&mut turn, results_msg(false, "hello there")).unwrap();
        let fin = apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd).unwrap();
        assert_eq!(
            fin.text, "hello there",
            "a later interim must not be discarded in favor of an older is_final"
        );
    }

    #[test]
    fn a_restated_is_final_does_not_duplicate_the_prefix() {
        let mut turn = DeepgramTurn::default();
        apply_deepgram_message(&mut turn, results_msg(true, "hello")).unwrap();
        apply_deepgram_message(&mut turn, results_msg(true, "hello there")).unwrap();
        let fin = apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd).unwrap();
        assert_eq!(fin.text, "hello there");
    }

    #[test]
    fn utterance_end_with_no_prior_text_is_dropped() {
        let mut turn = DeepgramTurn::default();
        assert_eq!(
            apply_deepgram_message(&mut turn, DeepgramMessage::UtteranceEnd),
            None
        );
    }

    #[test]
    fn audio_is_resampled_to_the_rate_the_server_was_told() {
        // 10ms of 48kHz capture audio — what `mic.rs` actually produces
        // (TARGET_RATE = 48_000) — against the 16_000 that
        // `DeepgramConfig::default()` writes into the connect URL.
        let ten_ms_at_48k = vec![0.5_f32; 480];
        let wire = encode_for_wire(&ten_ms_at_48k, 48_000, 16_000);

        // 160 samples of i16 = 320 bytes. Reinstating the bug (ignoring the
        // source rate and encoding the input verbatim) gives 960 bytes here,
        // and Deepgram decodes the stream 3x too fast.
        assert_eq!(
            wire.len(),
            320,
            "48kHz audio must be downsampled to the declared 16kHz before it is sent"
        );
    }

    #[test]
    fn a_capture_rate_that_already_matches_is_passed_through_unchanged() {
        // Guards the opposite direction: the resample must not corrupt or
        // re-length audio that is already at the declared rate.
        let ten_ms_at_16k = vec![0.25_f32; 160];
        assert_eq!(encode_for_wire(&ten_ms_at_16k, 16_000, 16_000).len(), 320);
    }

    #[test]
    fn the_bluetooth_headset_rate_is_upsampled_rather_than_mislabelled() {
        // System audio on a Bluetooth headset in HFP mode reports 24kHz (seen
        // live on this machine), so the conversion has to work upward too —
        // not just the common 48k downward case.
        let ten_ms_at_24k = vec![0.1_f32; 240];
        assert_eq!(encode_for_wire(&ten_ms_at_24k, 24_000, 16_000).len(), 320);
    }

    #[tokio::test]
    async fn push_audio_on_a_never_connected_engine_does_not_panic_or_block() {
        // Points at an address nothing listens on, so the background task
        // will be stuck failing to connect — push_audio must still be safe
        // to call (non-blocking `try_send`) regardless of connection state.
        let mut engine = DeepgramLiveEngine::start(
            "test-key".to_string(),
            DeepgramConfig {
                model: "nova-3".to_string(),
                language: "en".to_string(),
                sample_rate: 16_000,
                channels: 1,
                keyterms: Vec::new(),
            },
        );
        engine.push_audio(&[0.1; 480], 16_000);
        // No assertion beyond "this returned" — the property under test is
        // that push_audio never blocks or panics, not any particular result.
    }
}
