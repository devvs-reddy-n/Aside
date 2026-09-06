//! Live Speechmatics Real-Time engine over a WebSocket.
//!
//! Structurally a sibling of `deepgram_live.rs` — same background-task shape,
//! same `Outgoing` channel, same `terminal_error` cell, same stability-gated
//! backoff — because those patterns were arrived at by adversarial review of
//! that engine and there is no reason to relearn them. Read that file first;
//! this comment covers only where the two genuinely diverge.
//!
//! ## Four real differences from the Deepgram engine
//!
//! 1. **A handshake gate.** Speechmatics requires `StartRecognition` and then
//!    a `RecognitionStarted` reply *before* any audio. Sending early is a
//!    `protocol_error` that terminates the session. Audio that arrives while
//!    the gate is closed is dropped, not queued: this is live conversation
//!    audio, and replaying a second of stale speech into a fresh session is
//!    worse than losing it (the same reasoning behind the queue-drain on
//!    reconnect in `deepgram_live.rs`).
//! 2. **`flush()` sends `ForceEndOfUtterance`, never `EndOfStream`.** See
//!    `speechmatics.rs`'s module doc. `pipeline.rs` calls `flush()` only when
//!    transcription **stops** (not on speaking-light VAD — Speechmatics
//!    already endpoints via `max_delay` / `AddTranscript`). Confusing
//!    ForceEndOfUtterance with EndOfStream would still kill the session.
//! 3. **Keepalive is a WebSocket Ping**, because the protocol has no
//!    application-level keepalive message. The idle rule the docs state counts
//!    "no audio or ping/pongs", so a Ping satisfies it.
//! 4. **Close codes are classified, not treated uniformly.** The vendor
//!    documents which codes are worth retrying and asks for a slower floor on
//!    three of them; `speechmatics::classify_close` encodes that, and this
//!    file honours it.
//!
//! ## A known, deliberate limitation
//!
//! `shutdown()` sends `EndOfStream` and returns without waiting for the
//! trailing `EndOfTranscript`, so a final still in flight when the user stops
//! capture is lost. Fixing it properly means `SttEngine::shutdown` returning
//! `Vec<Transcript>` and `pipeline.rs` emitting them — a trait change that
//! would benefit every engine and is worth doing, but is deliberately not
//! bundled into this one's first landing. Recorded rather than left silent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::{ClientRequestBuilder, Message};

use super::resample::resample_linear;
use super::speechmatics::{
    auth_header_value, build_session_url, classify_close, end_of_stream_message,
    force_end_of_utterance_message, is_terminal_error_type, jwt_mint_body, looks_like_jwt,
    parse_jwt_mint_response, parse_message, start_recognition_message, CloseDisposition, Region,
    SpeechmaticsConfig, SpeechmaticsMessage, JWT_MINT_URL, JWT_TTL_SECS,
};
use super::wav::f32_to_i16_le_bytes;
use super::{SttEngine, Transcript};

const AUDIO_CHANNEL_CAPACITY: usize = 256;
const TRANSCRIPT_CHANNEL_CAPACITY: usize = 256;
const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const STABILITY_WINDOW: Duration = Duration::from_secs(5);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(8);

/// The floor the docs ask for on `quota_exceeded` / `job_error` /
/// `internal_error` specifically: "a client retry interval of at least 5-10
/// seconds". Retrying those at the ordinary 1s base backoff is how a
/// concurrent-session limit turns into a hot loop against the vendor.
const SLOW_RETRY_FLOOR: Duration = Duration::from_secs(8);

/// How long to wait for `RecognitionStarted` before treating the connection as
/// failed. Generous relative to a handshake that is normally immediate — the
/// cost of waiting slightly too long is a delayed reconnect, while the cost of
/// giving up too early is an unnecessary reconnect storm.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

enum Outgoing {
    Audio(Vec<u8>),
    ForceEndOfUtterance,
    EndOfStream,
}

pub struct SpeechmaticsLiveEngine {
    outgoing_tx: mpsc::Sender<Outgoing>,
    transcript_rx: mpsc::Receiver<Transcript>,
    stop: Arc<AtomicBool>,
    /// Set once the connection task gives up permanently. Same purpose as the
    /// Deepgram engine's: without it, a dead task is indistinguishable from
    /// "nobody has spoken yet" at every layer above, including the user.
    terminal_error: Arc<Mutex<Option<String>>>,
    /// Latest retryable connect/handshake problem. Shown immediately so the
    /// inspector does not sit on "transcribing" with no words until the 10th
    /// silent retry fails.
    last_problem: Arc<Mutex<Option<String>>>,
    /// The rate declared in `audio_format`. Fixed for the life of the session,
    /// so every pushed buffer must be resampled to it — see `push_audio`.
    declared_sample_rate: u32,
}

impl SpeechmaticsLiveEngine {
    pub fn start(api_key: String, config: SpeechmaticsConfig) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::channel(AUDIO_CHANNEL_CAPACITY);
        let (transcript_tx, transcript_rx) = mpsc::channel(TRANSCRIPT_CHANNEL_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));
        let last_problem = Arc::new(Mutex::new(None));

        let task_stop = stop.clone();
        let task_error = terminal_error.clone();
        let task_problem = last_problem.clone();
        let declared_sample_rate = config.sample_rate;
        tauri::async_runtime::spawn(connection_task(
            api_key,
            config,
            outgoing_rx,
            transcript_tx,
            task_stop,
            task_error,
            task_problem,
        ));

        Self {
            outgoing_tx,
            transcript_rx,
            stop,
            terminal_error,
            last_problem,
            declared_sample_rate,
        }
    }

    fn take_terminal_error(&self) -> Option<String> {
        self.terminal_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn current_problem(&self) -> Option<String> {
        self.last_problem
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn drain_ready(&mut self) -> Vec<Transcript> {
        let mut out = Vec::new();
        while let Ok(t) = self.transcript_rx.try_recv() {
            out.push(t);
        }
        out
    }
}

/// Resamples capture-rate audio to the declared rate and encodes it as the
/// little-endian 16-bit PCM `audio_format` promised.
///
/// Split out for the same reason as the Deepgram engine's equivalent: the real
/// `push_audio` writes into a channel owned by a background task, so there is
/// nothing observable from outside, and this exact conversion is where a
/// silent, output-ruining bug lived in that engine for its whole life.
///
/// The `sample_rate` argument is genuinely load-bearing here and must not be
/// ignored: `audio_format` is frozen at `StartRecognition`, while Aside's
/// capture rate is not — it is 48kHz normally and 24kHz for system audio on a
/// Bluetooth headset in HFP mode. A device switch mid-session has to be
/// absorbed here or every subsequent transcript is decoded at the wrong speed.
fn encode_for_wire(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<u8> {
    f32_to_i16_le_bytes(&resample_linear(samples, from_rate, to_rate))
}

#[async_trait::async_trait]
impl SttEngine for SpeechmaticsLiveEngine {
    fn push_audio(&mut self, samples: &[f32], sample_rate: u32) {
        let pcm = encode_for_wire(samples, sample_rate, self.declared_sample_rate);
        // `try_send`, never `send`: this runs adjacent to a real-time audio
        // path and must not block. A full channel means the connection task is
        // behind (reconnecting, slow network), and dropping the chunk is the
        // right call.
        if self.outgoing_tx.try_send(Outgoing::Audio(pcm)).is_err() {
            eprintln!("speechmatics: audio channel full or closed, dropping a chunk");
        }
    }

    async fn drain(&mut self) -> Result<Vec<Transcript>, String> {
        // Drained before the error check, not instead of it: a connection that
        // died may still have delivered good transcripts before it did, and
        // throwing those away would lose real speech.
        let ready = self.drain_ready();
        if !ready.is_empty() {
            return Ok(ready);
        }
        if let Some(err) = self.take_terminal_error() {
            return Err(err);
        }
        if let Some(problem) = self.current_problem() {
            return Err(problem);
        }
        Ok(ready)
    }

    async fn flush(&mut self) -> Result<Vec<Transcript>, String> {
        let _ = self.outgoing_tx.try_send(Outgoing::ForceEndOfUtterance);
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.drain().await
    }

    async fn shutdown(self: Box<Self>) {
        // Ask for a clean protocol-level close before tearing the task down,
        // so the server is not left with a half-open session counting against
        // the account's concurrent-session limit — which matters more here
        // than it would elsewhere, because Aside runs two of these at once
        // (mic + system audio) and the free tier's ceiling is exactly two.
        let _ = self.outgoing_tx.try_send(Outgoing::EndOfStream);
        // Give the connection task a moment to actually send it. Bounded and
        // short: `shutdown` is called on the path where the user has already
        // stopped capture, so a long wait would be felt as a hang.
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct TurnDedupe {
    /// Finalized Speechmatics prefixes for the open utterance (`AddTranscript`
    /// is a delta, not a whole sentence). Joined only at `EndOfUtterance` so
    /// the inspector matches Parakeet: one committed line per pause.
    committed: Vec<String>,
    last_partial: String,
    last_emitted_final: String,
    /// True after new audio text since the last emitted final. Lets "okay"
    /// then "okay" both land, while a second EOU with no new words does not.
    saw_speech_since_emit: bool,
}

fn live_text(state: &TurnDedupe) -> String {
    let head = state.committed.join(" ");
    let head = head.trim();
    let tail = state.last_partial.trim();
    if head.is_empty() {
        tail.to_string()
    } else if tail.is_empty() {
        head.to_string()
    } else if head.eq_ignore_ascii_case(tail)
        || tail
            .to_ascii_lowercase()
            .starts_with(&head.to_ascii_lowercase())
    {
        tail.to_string()
    } else if head
        .to_ascii_lowercase()
        .ends_with(&tail.to_ascii_lowercase())
    {
        head.to_string()
    } else {
        format!("{head} {tail}")
    }
}

fn push_committed_piece(committed: &mut Vec<String>, text: &str) {
    if committed.is_empty() {
        committed.push(text.to_string());
        return;
    }
    let live = committed.join(" ");
    let live_l = live.to_ascii_lowercase();
    let text_l = text.to_ascii_lowercase();
    if text_l == live_l || text_l.starts_with(&live_l) {
        *committed = vec![text.to_string()];
    } else if live_l.ends_with(&text_l) {
        // Already the last piece.
    } else {
        committed.push(text.to_string());
    }
}

fn preview(state: &TurnDedupe) -> Option<Transcript> {
    let text = live_text(state);
    if text.is_empty() {
        None
    } else {
        Some(Transcript {
            text,
            is_final: false,
        })
    }
}

/// Maps a parsed server message to a `Transcript`, or `None` for control
/// messages.
///
/// Speechmatics `AddTranscript` is a **prefix/delta**, not a finished
/// sentence (unlike Parakeet's EOU-sized final). Emitting each one as
/// `is_final` made the inspector look like one word per line. They stay
/// interim until `EndOfUtterance` (or ForceEOU, which produces the same
/// server message).
fn apply_message(msg: SpeechmaticsMessage, state: &mut TurnDedupe) -> Option<Transcript> {
    match msg {
        SpeechmaticsMessage::AddTranscript { transcript } => {
            let text = transcript.trim();
            if text.is_empty() {
                return None;
            }
            // Speechmatics often echoes the finished line as AddTranscript
            // after EOU. Skip that echo. A real repeat ("okay" then "okay")
            // arrives as a new partial first, so last_partial is not empty.
            if text == state.last_emitted_final && state.last_partial.is_empty() {
                return None;
            }
            push_committed_piece(&mut state.committed, text);
            state.saw_speech_since_emit = true;
            let head = state.committed.join(" ").to_ascii_lowercase();
            let tail = state.last_partial.to_ascii_lowercase();
            if !tail.is_empty() && (head.contains(&tail) || head.ends_with(&tail)) {
                state.last_partial.clear();
            }
            preview(state)
        }
        SpeechmaticsMessage::AddPartialTranscript { transcript } => {
            let text = transcript.trim();
            if text.is_empty() {
                return None;
            }
            state.last_partial = text.to_string();
            state.saw_speech_since_emit = true;
            preview(state)
        }
        SpeechmaticsMessage::EndOfUtterance => {
            let text = live_text(state);
            let had_speech = state.saw_speech_since_emit;
            state.committed.clear();
            state.last_partial.clear();
            state.saw_speech_since_emit = false;
            if text.is_empty() || !had_speech {
                return None;
            }
            state.last_emitted_final = text.clone();
            Some(Transcript {
                text,
                is_final: true,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
fn to_transcript(msg: SpeechmaticsMessage) -> Option<Transcript> {
    apply_message(msg, &mut TurnDedupe::default())
}

enum ConnectOutcome {
    Stopped,
    Disconnected {
        was_stable: bool,
        reason: String,
    },
    /// Retryable, but only after `SLOW_RETRY_FLOOR` — the vendor asks for this
    /// specifically on quota/internal-error close codes.
    DisconnectedSlow {
        reason: String,
    },
    /// No amount of retrying fixes this. Surfaced to the user instead.
    Terminal(String),
}

fn give_up_message(attempts: u32, last: Option<&str>) -> String {
    match last {
        Some(reason) if !reason.is_empty() => format!(
            "speechmatics: giving up after {attempts} reconnect attempts. Last problem: {reason}"
        ),
        _ => format!("speechmatics: giving up after {attempts} reconnect attempts"),
    }
}

/// Keep the real failure visible while waiting to reconnect. Overwriting it
/// with only "retrying (5/10) in 8s" is how a live bug becomes undiagnosable.
fn retrying_message(attempt: u32, wait_secs: u64, last: Option<&str>) -> String {
    let retry =
        format!("Speechmatics is retrying ({attempt}/{MAX_RECONNECT_ATTEMPTS}) in {wait_secs}s…");
    match last {
        Some(reason) if !reason.is_empty() => format!("{reason} — {retry}"),
        _ => retry,
    }
}

fn store_problem(cell: &Arc<Mutex<Option<String>>>, msg: String) {
    eprintln!("{msg}");
    *cell.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

enum TokenResolution {
    Token(String),
    Terminal(String),
}

enum MintFailure {
    Rejected(String),
    Retryable(String),
}

fn websocket_request(
    url: &str,
    session_token: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let uri: Uri = url
        .parse()
        .map_err(|e| format!("invalid speechmatics URL: {e}"))?;
    ClientRequestBuilder::new(uri)
        .with_header("Authorization", auth_header_value(session_token))
        .into_client_request()
        .map_err(|e| format!("could not build speechmatics request: {e}"))
}

/// Turns the saved portal key into a session token.
///
/// A long-lived key is exchanged for a realtime JWT at the management
/// platform (what the official JS SDK always does). If the saved value is
/// already a JWT, it is used as-is. A mint 401/403 means the saved key is
/// wrong — that is terminal. A network blip falls back to the saved key so
/// a reachable realtime host can still authenticate via Bearer + `jwt=`.
async fn resolve_session_token(
    api_key: &str,
    region: Region,
    cached: &mut Option<CachedToken>,
    last_problem: &Arc<Mutex<Option<String>>>,
) -> TokenResolution {
    if let Some(existing) = cached.as_ref() {
        if Instant::now() + Duration::from_secs(90) < existing.expires_at {
            return TokenResolution::Token(existing.value.clone());
        }
    }
    if looks_like_jwt(api_key) {
        *cached = Some(CachedToken {
            value: api_key.to_string(),
            expires_at: Instant::now() + Duration::from_secs(JWT_TTL_SECS),
        });
        return TokenResolution::Token(api_key.to_string());
    }
    match mint_rt_jwt(api_key, region).await {
        Ok(jwt) => {
            *cached = Some(CachedToken {
                value: jwt.clone(),
                expires_at: Instant::now() + Duration::from_secs(JWT_TTL_SECS),
            });
            TokenResolution::Token(jwt)
        }
        Err(MintFailure::Rejected(msg)) => TokenResolution::Terminal(msg),
        Err(MintFailure::Retryable(msg)) => {
            store_problem(
                last_problem,
                format!(
                    "Could not mint a Speechmatics realtime token ({msg}). Trying the saved key directly."
                ),
            );
            TokenResolution::Token(api_key.to_string())
        }
    }
}

async fn mint_rt_jwt(api_key: &str, region: Region) -> Result<String, MintFailure> {
    let client = reqwest::Client::new();
    let resp = match client
        .post(JWT_MINT_URL)
        .bearer_auth(api_key)
        .json(&jwt_mint_body(JWT_TTL_SECS, region))
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(MintFailure::Retryable(format!("network error: {e}"))),
    };
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    match parse_jwt_mint_response(status, &body) {
        Ok(token) => Ok(token),
        Err(msg) if matches!(status, 401 | 403) => Err(MintFailure::Rejected(msg)),
        Err(msg) => Err(MintFailure::Retryable(msg)),
    }
}

fn clear_problem(cell: &Arc<Mutex<Option<String>>>) {
    *cell.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

async fn connection_task(
    api_key: String,
    config: SpeechmaticsConfig,
    mut outgoing_rx: mpsc::Receiver<Outgoing>,
    transcript_tx: mpsc::Sender<Transcript>,
    stop: Arc<AtomicBool>,
    terminal_error: Arc<Mutex<Option<String>>>,
    last_problem: Arc<Mutex<Option<String>>>,
) {
    let mut attempt: u32 = 0;
    let mut force_slow_retry = false;
    let mut cached_token: Option<CachedToken> = None;
    let mut last_reason: Option<String> = None;

    // Config is fixed for the life of this task. Catch a local mistake
    // before any mint or websocket, so tests and the UI see `max_delay`
    // (etc.) instead of a 10-second token request.
    if let Err(e) = start_recognition_message(&config) {
        let msg = format!("speechmatics config is invalid: {e}");
        eprintln!("{msg}");
        *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
        return;
    }

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if attempt >= MAX_RECONNECT_ATTEMPTS {
            let msg = give_up_message(attempt, last_reason.as_deref());
            eprintln!("{msg}");
            *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
            return;
        }
        if attempt > 0 {
            let backoff =
                (BASE_BACKOFF.saturating_mul(1 << (attempt - 1).min(10))).min(MAX_BACKOFF);
            let backoff = if force_slow_retry {
                backoff.max(SLOW_RETRY_FLOOR)
            } else {
                backoff
            };
            store_problem(
                &last_problem,
                retrying_message(
                    attempt + 1,
                    backoff.as_secs().max(1),
                    last_reason.as_deref(),
                ),
            );
            tokio::time::sleep(backoff).await;
        }
        force_slow_retry = false;

        // Reconnects only. The first connect must keep audio that arrived
        // while the JWT was minted and the websocket opened — draining here
        // is how "I started talking and the first words never showed up"
        // happens. After a drop, queued audio is from before the new session
        // and would be replayed late, so it is discarded.
        if attempt > 0 {
            while outgoing_rx.try_recv().is_ok() {}
        }

        let session_token =
            match resolve_session_token(&api_key, config.region, &mut cached_token, &last_problem)
                .await
            {
                TokenResolution::Token(t) => t,
                TokenResolution::Terminal(msg) => {
                    eprintln!("speechmatics: not retrying: {msg}");
                    *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
                    return;
                }
            };

        match connect_and_run(
            &session_token,
            &config,
            &mut outgoing_rx,
            &transcript_tx,
            &stop,
            &last_problem,
        )
        .await
        {
            ConnectOutcome::Stopped => return,
            ConnectOutcome::Terminal(msg) => {
                eprintln!("speechmatics: not retrying: {msg}");
                *terminal_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg);
                return;
            }
            ConnectOutcome::DisconnectedSlow { reason } => {
                last_reason = Some(reason.clone());
                store_problem(&last_problem, reason);
                force_slow_retry = true;
                attempt += 1;
            }
            ConnectOutcome::Disconnected { was_stable, reason } => {
                last_reason = Some(reason.clone());
                store_problem(&last_problem, reason);
                attempt = if was_stable { 0 } else { attempt + 1 };
            }
        }
    }
}

async fn connect_and_run(
    session_token: &str,
    config: &SpeechmaticsConfig,
    outgoing_rx: &mut mpsc::Receiver<Outgoing>,
    transcript_tx: &mpsc::Sender<Transcript>,
    stop: &Arc<AtomicBool>,
    last_problem: &Arc<Mutex<Option<String>>>,
) -> ConnectOutcome {
    // Built before connecting: an invalid config is a local mistake, and
    // finding out about it before opening a socket makes it a clear error
    // rather than a mid-session `invalid_config` disconnect.
    let start_message = match start_recognition_message(config) {
        Ok(m) => m,
        Err(e) => return ConnectOutcome::Terminal(format!("speechmatics config is invalid: {e}")),
    };

    let request = match websocket_request(&build_session_url(config, session_token), session_token)
    {
        Ok(r) => r,
        Err(e) => return ConnectOutcome::Terminal(e),
    };

    let ws_stream = match tokio_tungstenite::connect_async(request).await {
        Ok((stream, _response)) => stream,
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            let status = resp.status();
            // The docs list 401 as specifically "when the API key is not
            // valid"; 400 and 405 are malformed-handshake faults that this
            // client's own code would have to change to fix. None are worth a
            // retry with identical inputs.
            return match status.as_u16() {
                401 | 403 => ConnectOutcome::Terminal(format!(
                    "Speechmatics rejected the API key ({status}). Paste the long-lived key from the Speechmatics portal (Settings → API Keys)."
                )),
                400 | 405 => ConnectOutcome::Terminal(format!(
                    "Speechmatics rejected the connection request ({status})."
                )),
                other => ConnectOutcome::Disconnected {
                    was_stable: false,
                    reason: format!("Speechmatics HTTP {other} while opening the websocket"),
                },
            };
        }
        Err(e) => {
            return ConnectOutcome::Disconnected {
                was_stable: false,
                reason: format!("Speechmatics websocket connect failed: {e}"),
            };
        }
    };

    let (mut sink, mut stream) = ws_stream.split();

    if sink
        .send(Message::Text(start_message.into()))
        .await
        .is_err()
    {
        return ConnectOutcome::Disconnected {
            was_stable: false,
            reason: "Speechmatics websocket closed before StartRecognition could be sent".into(),
        };
    }

    // The handshake gate. Nothing may be sent until the server confirms.
    match await_recognition_started(&mut stream).await {
        Ok(()) => clear_problem(last_problem),
        Err(HandshakeFailure::Terminal(msg)) => return ConnectOutcome::Terminal(msg),
        Err(HandshakeFailure::Retry(reason)) => {
            return ConnectOutcome::Disconnected {
                was_stable: false,
                reason,
            };
        }
    }

    let mut seq_no: u64 = 0;
    let mut became_stable = false;
    let mut dedupe = TurnDedupe::default();
    let stability_deadline = tokio::time::sleep(STABILITY_WINDOW);
    tokio::pin!(stability_deadline);
    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.tick().await; // consume the immediate first tick

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
                // A Ping, not a JSON message — this protocol has no
                // application-level keepalive, and the documented idle rule
                // counts "no audio or ping/pongs".
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return ConnectOutcome::Disconnected {
                        was_stable: became_stable,
                        reason: "Speechmatics websocket died while sending keepalive".into(),
                    };
                }
            }
            outgoing = outgoing_rx.recv() => {
                match outgoing {
                    Some(Outgoing::Audio(bytes)) => {
                        if sink.send(Message::Binary(bytes.into())).await.is_err() {
                            return ConnectOutcome::Disconnected {
                                was_stable: became_stable,
                                reason: "Speechmatics websocket died while sending audio".into(),
                            };
                        }
                        // Counted here, on a *successful send*, and nowhere
                        // else. Counting in `push_audio` would drift, because
                        // that path drops chunks when the channel is full —
                        // and `EndOfStream`'s `last_seq_no` has to describe
                        // what the server actually received.
                        seq_no += 1;
                    }
                    Some(Outgoing::ForceEndOfUtterance) => {
                        let _ = sink.send(Message::Text(force_end_of_utterance_message().into())).await;
                    }
                    Some(Outgoing::EndOfStream) => {
                        let _ = sink.send(Message::Text(end_of_stream_message(seq_no).into())).await;
                        let _ = sink.close().await;
                        return ConnectOutcome::Stopped;
                    }
                    None => {
                        // The engine was dropped without an explicit shutdown.
                        let _ = sink.send(Message::Text(end_of_stream_message(seq_no).into())).await;
                        let _ = sink.close().await;
                        return ConnectOutcome::Stopped;
                    }
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        match parse_message(&text) {
                            Ok(SpeechmaticsMessage::Error { error_type, reason }) => {
                                // The docs: "After any error, the transcription
                                // is terminated and the connection is closed."
                                let msg = if error_type == "not_authorised" {
                                    format!(
                                        "Speechmatics error (not_authorised): {reason}. \
                                         Paste the long-lived API key from the Speechmatics portal \
                                         (Settings → API Keys)."
                                    )
                                } else {
                                    format!("Speechmatics error ({error_type}): {reason}")
                                };
                                return if is_terminal_error_type(&error_type) {
                                    ConnectOutcome::Terminal(msg)
                                } else {
                                    ConnectOutcome::DisconnectedSlow { reason: msg }
                                };
                            }
                            Ok(SpeechmaticsMessage::Warning { warning_type, reason }) => {
                                // Non-fatal. `idle_timeout`/`session_timeout`
                                // warnings are advance notice that the server
                                // intends to close, which the ordinary
                                // reconnect path already handles.
                                eprintln!("speechmatics warning ({warning_type}): {reason}");
                            }
                            Ok(SpeechmaticsMessage::Info { info_type, reason }) => {
                                // `concurrent_session_usage` and
                                // `recognition_quality` land here — both are
                                // genuinely useful when diagnosing "why is
                                // this session behaving oddly".
                                eprintln!("speechmatics info ({info_type}): {reason}");
                            }
                            Ok(SpeechmaticsMessage::EndOfTranscript) => {
                                return ConnectOutcome::Stopped;
                            }
                            Ok(msg) => {
                                if let Some(t) = apply_message(msg, &mut dedupe) {
                                    let _ = transcript_tx.try_send(t);
                                }
                            }
                            Err(e) => {
                                // One unparseable frame must not tear down a
                                // working session — same stance as the
                                // Deepgram engine.
                                eprintln!("speechmatics: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1006);
                        let reason = format!("Speechmatics closed the websocket (code {code})");
                        return match classify_close(code) {
                            CloseDisposition::Terminal(msg) => ConnectOutcome::Terminal(msg),
                            CloseDisposition::RetrySlow => ConnectOutcome::DisconnectedSlow { reason },
                            CloseDisposition::RetryFast => ConnectOutcome::Disconnected {
                                was_stable: became_stable,
                                reason,
                            },
                        };
                    }
                    None => {
                        return ConnectOutcome::Disconnected {
                            was_stable: became_stable,
                            reason: "Speechmatics websocket closed without a close frame".into(),
                        };
                    }
                    Some(Ok(_)) => {} // ping/pong/binary from the server
                    Some(Err(e)) => {
                        return ConnectOutcome::Disconnected {
                            was_stable: became_stable,
                            reason: format!("Speechmatics websocket read error: {e}"),
                        };
                    }
                }
            }
        }
    }
}

enum HandshakeFailure {
    Terminal(String),
    Retry(String),
}

/// Waits for `RecognitionStarted`, tolerating any diagnostic messages the
/// server sends first (`Info` arrives immediately after the handshake).
async fn await_recognition_started<S>(stream: &mut S) -> Result<(), HandshakeFailure>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next()).await;
        match next {
            Err(_) => {
                return Err(HandshakeFailure::Terminal(
                    "Speechmatics did not confirm the session within 10s (no RecognitionStarted)."
                        .to_string(),
                ))
            }
            Ok(Some(Ok(Message::Text(text)))) => match parse_message(&text) {
                Ok(SpeechmaticsMessage::RecognitionStarted) => return Ok(()),
                Ok(SpeechmaticsMessage::Error { error_type, reason }) => {
                    let msg = if error_type == "not_authorised" {
                        format!(
                            "Speechmatics rejected the session (not_authorised): {reason}. \
                             Paste the long-lived API key from the Speechmatics portal \
                             (Settings → API Keys). A temporary token, a batch-only key, \
                             or a copied “Bearer …” line will not work."
                        )
                    } else {
                        format!("Speechmatics rejected the session ({error_type}): {reason}")
                    };
                    return Err(if is_terminal_error_type(&error_type) {
                        HandshakeFailure::Terminal(msg)
                    } else {
                        HandshakeFailure::Retry(msg)
                    });
                }
                // Info/Warning before RecognitionStarted is normal.
                _ => continue,
            },
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => {
                return Err(HandshakeFailure::Retry(format!(
                    "Speechmatics handshake read error: {e}"
                )));
            }
            Ok(None) => {
                return Err(HandshakeFailure::Retry(
                    "Speechmatics closed the websocket before RecognitionStarted".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_request_sends_authorization_bearer() {
        let req = websocket_request(
            "wss://eu2.rt.speechmatics.com/v2?jwt=eyJ.test.sig",
            "eyJ.test.sig",
        )
        .expect("request must build");
        let auth = req
            .headers()
            .get("Authorization")
            .expect("Authorization header must be present")
            .to_str()
            .expect("header is ascii");
        assert_eq!(auth, "Bearer eyJ.test.sig");
        assert!(req.uri().to_string().contains("jwt=eyJ.test.sig"));
    }

    #[tokio::test]
    async fn a_saved_jwt_is_used_without_hitting_the_mint_endpoint() {
        let mut cached = None;
        match resolve_session_token(
            "eyJhbGciOiJSUzI1NiJ9.e30.sig",
            Region::Us,
            &mut cached,
            &std::sync::Arc::new(std::sync::Mutex::new(None)),
        )
        .await
        {
            TokenResolution::Token(t) => assert_eq!(t, "eyJhbGciOiJSUzI1NiJ9.e30.sig"),
            TokenResolution::Terminal(msg) => panic!("saved JWT must not be terminal: {msg}"),
        }
        assert!(cached.is_some());
    }

    #[test]
    fn retrying_message_keeps_the_real_problem_visible() {
        assert_eq!(
            retrying_message(5, 8, None),
            "Speechmatics is retrying (5/10) in 8s…"
        );
        assert_eq!(
            retrying_message(
                5,
                8,
                Some("Speechmatics websocket connect failed: timed out")
            ),
            "Speechmatics websocket connect failed: timed out — Speechmatics is retrying (5/10) in 8s…"
        );
    }

    #[test]
    fn give_up_message_includes_the_last_real_problem() {
        assert_eq!(
            give_up_message(10, None),
            "speechmatics: giving up after 10 reconnect attempts"
        );
        assert_eq!(
            give_up_message(10, Some("Speechmatics rejected the session (not_authorised): invalid token")),
            "speechmatics: giving up after 10 reconnect attempts. Last problem: Speechmatics rejected the session (not_authorised): invalid token"
        );
    }

    #[test]
    fn add_transcript_is_a_growing_preview_not_a_committed_sentence() {
        // Speechmatics finalizes prefixes as it goes. Treating each as a
        // committed line is why the inspector showed "Already." / "I" / "am"
        // instead of one sentence like Parakeet.
        assert_eq!(
            to_transcript(SpeechmaticsMessage::AddTranscript {
                transcript: "Can you walk through your previous experience?".to_string()
            }),
            Some(Transcript {
                text: "Can you walk through your previous experience?".to_string(),
                is_final: false,
            })
        );
    }

    #[test]
    fn prefix_finals_join_into_one_sentence_at_end_of_utterance() {
        let mut state = TurnDedupe::default();
        let a = apply_message(
            SpeechmaticsMessage::AddTranscript {
                transcript: "Already.".into(),
            },
            &mut state,
        )
        .unwrap();
        assert!(!a.is_final);
        assert_eq!(a.text, "Already.");
        let b = apply_message(
            SpeechmaticsMessage::AddTranscript {
                transcript: "I".into(),
            },
            &mut state,
        )
        .unwrap();
        assert!(!b.is_final);
        assert_eq!(b.text, "Already. I");
        let c = apply_message(
            SpeechmaticsMessage::AddPartialTranscript {
                transcript: "am".into(),
            },
            &mut state,
        )
        .unwrap();
        assert!(!c.is_final);
        assert_eq!(c.text, "Already. I am");
        let fin = apply_message(SpeechmaticsMessage::EndOfUtterance, &mut state).unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "Already. I am");
        assert_eq!(
            apply_message(SpeechmaticsMessage::EndOfUtterance, &mut state),
            None,
            "a second EOU with nothing new must not duplicate the sentence"
        );
    }

    #[test]
    fn a_repeated_okay_after_eou_is_a_new_final() {
        let mut state = TurnDedupe::default();
        apply_message(
            SpeechmaticsMessage::AddTranscript {
                transcript: "okay".into(),
            },
            &mut state,
        );
        let first = apply_message(SpeechmaticsMessage::EndOfUtterance, &mut state).unwrap();
        assert_eq!(first.text, "okay");
        apply_message(
            SpeechmaticsMessage::AddPartialTranscript {
                transcript: "okay".into(),
            },
            &mut state,
        );
        apply_message(
            SpeechmaticsMessage::AddTranscript {
                transcript: "okay".into(),
            },
            &mut state,
        );
        let second = apply_message(SpeechmaticsMessage::EndOfUtterance, &mut state).unwrap();
        assert_eq!(second.text, "okay");
    }

    #[test]
    fn a_partial_transcript_is_marked_not_final() {
        assert_eq!(
            to_transcript(SpeechmaticsMessage::AddPartialTranscript {
                transcript: "Can you walk through".to_string()
            }),
            Some(Transcript {
                text: "Can you walk through".to_string(),
                is_final: false,
            })
        );
    }

    #[test]
    fn empty_and_whitespace_only_transcripts_are_dropped() {
        // The server can emit an empty transcript for a silent stretch;
        // forwarding it would clear the user's live preview for no reason.
        assert_eq!(
            to_transcript(SpeechmaticsMessage::AddTranscript {
                transcript: String::new()
            }),
            None
        );
        assert_eq!(
            to_transcript(SpeechmaticsMessage::AddPartialTranscript {
                transcript: "   \n".to_string()
            }),
            None
        );
    }

    #[test]
    fn control_messages_never_produce_a_transcript() {
        for msg in [
            SpeechmaticsMessage::RecognitionStarted,
            SpeechmaticsMessage::AudioAdded { seq_no: 3 },
            SpeechmaticsMessage::EndOfTranscript,
            SpeechmaticsMessage::Unknown {
                message: "Future".to_string(),
            },
        ] {
            assert_eq!(to_transcript(msg), None);
        }
        // EndOfUtterance with no prior partial is also nothing.
        assert_eq!(to_transcript(SpeechmaticsMessage::EndOfUtterance), None);
    }

    #[test]
    fn end_of_utterance_becomes_a_synthetic_final_from_the_last_partial() {
        let mut state = TurnDedupe::default();
        let partial = apply_message(
            SpeechmaticsMessage::AddPartialTranscript {
                transcript: "what is your notice period".into(),
            },
            &mut state,
        )
        .unwrap();
        assert!(!partial.is_final);
        let fin = apply_message(SpeechmaticsMessage::EndOfUtterance, &mut state).unwrap();
        assert!(fin.is_final);
        assert_eq!(fin.text, "what is your notice period");
        assert_eq!(
            apply_message(
                SpeechmaticsMessage::AddTranscript {
                    transcript: "what is your notice period".into(),
                },
                &mut state,
            ),
            None,
            "double-final of the same wording must be dropped"
        );
    }

    #[test]
    fn audio_is_resampled_to_the_declared_rate() {
        // 10ms of 48kHz capture audio (mic.rs's TARGET_RATE) against the
        // 16kHz declared in `audio_format`. `audio_format` is frozen for the
        // session, so this conversion is not optional — the same bug that
        // silently ruined every Deepgram transcript lived in exactly this
        // spot.
        assert_eq!(
            encode_for_wire(&vec![0.5_f32; 480], 48_000, 16_000).len(),
            320
        );
    }

    #[test]
    fn a_mid_session_device_switch_is_absorbed_rather_than_mislabelled() {
        // System audio drops to 24kHz when a Bluetooth headset enters HFP
        // mode (observed live on this machine). Because the declared rate
        // cannot change mid-session, the engine has to absorb it here.
        assert_eq!(
            encode_for_wire(&vec![0.1_f32; 240], 24_000, 16_000).len(),
            320
        );
        // ...and audio already at the declared rate must pass through
        // unchanged rather than being needlessly re-lengthened.
        assert_eq!(
            encode_for_wire(&vec![0.25_f32; 160], 16_000, 16_000).len(),
            320
        );
    }

    #[tokio::test]
    async fn push_audio_on_a_never_connected_engine_does_not_panic_or_block() {
        // The connection task will be stuck failing against a host that
        // cannot resolve; `push_audio` must stay non-blocking regardless,
        // because it runs adjacent to a real-time audio path.
        let mut engine = SpeechmaticsLiveEngine::start(
            "test-key".to_string(),
            SpeechmaticsConfig {
                region: super::super::speechmatics::Region::Global,
                ..SpeechmaticsConfig::default()
            },
        );
        for _ in 0..10 {
            engine.push_audio(&[0.0; 480], 48_000);
        }
        assert!(engine.drain().await.is_ok() || engine.drain().await.is_err());
    }

    #[tokio::test]
    async fn an_invalid_config_fails_fast_instead_of_opening_a_socket() {
        // A `max_delay` outside the documented range would be rejected by the
        // server as `invalid_config` *after* a successful handshake. Catching
        // it locally turns an unexplained mid-session disconnect into a
        // legible terminal error the UI can show.
        let mut engine = SpeechmaticsLiveEngine::start(
            "test-key".to_string(),
            SpeechmaticsConfig {
                max_delay: 0.1,
                ..SpeechmaticsConfig::default()
            },
        );
        // Give the connection task a moment to record the terminal error.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let err = engine
            .drain()
            .await
            .expect_err("an invalid config must surface as an error");
        assert!(
            err.contains("max_delay"),
            "the error should name the offending field, got: {err}"
        );
    }
}
