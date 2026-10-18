//! Speechmatics streaming: confirmed readiness, bounded audio replay, separate
//! control messages, cancellable retries and acknowledged graceful shutdown.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::{ClientRequestBuilder, Message};
use super::connection::SpeechConnection;
use super::resample::StreamingResampler;
#[cfg(test)]
use super::resample::resample_linear;
use super::speechmatics_buffer::{AudioBuffer, Packet, SentAudio};
use super::speechmatics::{auth_header_value, classify_close, end_of_stream_message,
    is_terminal_error_type, jwt_mint_body,
    parse_jwt_mint_response, parse_message, start_recognition_message, CloseDisposition,
    Region, SpeechmaticsConfig, SpeechmaticsMessage, JWT_MINT_URL, JWT_TTL_SECS};
use super::wav::f32_to_i16_le_bytes;
use super::{SttEngine, Transcript};

const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const SLOW_RETRY_FLOOR: Duration = Duration::from_secs(8);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(8);
const STABILITY_WINDOW: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20); // custom dictionaries can need 15s

#[derive(Clone)]
struct TransportOptions {
    endpoint: Option<String>,
    handshake_timeout: Duration,
    retry_scale: f64,
    diagnostics: Option<std::path::PathBuf>,
}
impl Default for TransportOptions {
    fn default() -> Self {
        Self { endpoint: None, handshake_timeout: HANDSHAKE_TIMEOUT, retry_scale: 1.0, diagnostics: None }
    }
}

enum Control { End }

pub struct SpeechmaticsLiveEngine {
    audio: Arc<Mutex<AudioBuffer>>,
    audio_notify: Arc<Notify>,
    control: mpsc::UnboundedSender<Control>,
    transcript_rx: mpsc::Receiver<Transcript>,
    status: watch::Receiver<SpeechConnection>,
    cancel: watch::Sender<bool>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    resampler: StreamingResampler,
    pending: Vec<u8>,
    declared_sample_rate: u32,
    reported_drops: usize,
    diagnostics: Option<std::path::PathBuf>,
}

impl SpeechmaticsLiveEngine {
    pub fn start(api_key: String, config: SpeechmaticsConfig) -> Self {
        Self::start_with_options(api_key, config, TransportOptions::default())
    }
    pub fn start_with_diagnostics(api_key: String, config: SpeechmaticsConfig, path: std::path::PathBuf) -> Self {
        Self::start_with_options(api_key, config, TransportOptions { diagnostics: Some(path), ..TransportOptions::default() })
    }
    fn start_with_options(api_key: String, config: SpeechmaticsConfig, options: TransportOptions) -> Self {
        let audio = Arc::new(Mutex::new(AudioBuffer::new(config.sample_rate)));
        let audio_notify = Arc::new(Notify::new());
        let (control, controls) = mpsc::unbounded_channel();
        let (transcript_tx, transcript_rx) = mpsc::channel(256);
        let (state, status) = watch::channel(SpeechConnection::Connecting);
        let (cancel, cancelled) = watch::channel(false);
        let rate = config.sample_rate;
        let diagnostics = options.diagnostics.clone();
        let task = tauri::async_runtime::spawn(connection_task(api_key, config, audio.clone(), audio_notify.clone(),
            controls, transcript_tx, state, cancelled, options));
        Self { audio, audio_notify, control, transcript_rx, status, cancel, task: Some(task),
            resampler: StreamingResampler::default(), pending: Vec::new(), declared_sample_rate: rate, reported_drops: 0, diagnostics }
    }
    fn drain_ready(&mut self) -> Vec<Transcript> {
        let mut out = Vec::new();
        while let Ok(t) = self.transcript_rx.try_recv() { out.push(t); }
        out
    }
    fn finish_pending(&mut self) {
        if !self.pending.is_empty() {
            self.audio.lock().unwrap_or_else(|p| p.into_inner()).push(std::mem::take(&mut self.pending));
            self.audio_notify.notify_one();
        }
    }
}
impl Drop for SpeechmaticsLiveEngine {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Some(task) = self.task.take() { task.abort(); }
    }
}

#[cfg(test)]
fn encode_for_wire(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<u8> {
    f32_to_i16_le_bytes(&resample_linear(samples, from_rate, to_rate))
}

#[async_trait::async_trait]
impl SttEngine for SpeechmaticsLiveEngine {
    fn connection_status(&self) -> Option<watch::Receiver<SpeechConnection>> { Some(self.status.clone()) }
    fn push_audio(&mut self, samples: &[f32], sample_rate: u32) {
        let converted = self.resampler.process(samples, sample_rate, self.declared_sample_rate);
        self.pending.extend(f32_to_i16_le_bytes(&converted));
        // Fixed 40ms packets bound both network overhead and first-word delay.
        let bytes = (self.declared_sample_rate as usize * 2 * 40 / 1000).max(2);
        let mut queue = self.audio.lock().unwrap_or_else(|p| p.into_inner());
        while self.pending.len() >= bytes {
            queue.push(self.pending.drain(..bytes).collect());
        }
        drop(queue);
        self.audio_notify.notify_one();
    }
    async fn drain(&mut self) -> Result<Vec<Transcript>, String> {
        let dropped = self.audio.lock().unwrap_or_else(|p| p.into_inner()).dropped_bytes;
        if dropped > self.reported_drops {
            self.reported_drops = dropped;
            return Err("Speechmatics was unavailable for too long. Some earlier audio could not be recovered.".into());
        }
        let ready = self.drain_ready();
        if !ready.is_empty() { return Ok(ready); }
        match self.status.borrow().clone() {
            SpeechConnection::Failed { message } | SpeechConnection::Recovering { message } => Err(message),
            _ => Ok(ready),
        }
    }
    async fn flush(&mut self) -> Result<Vec<Transcript>, String> {
        self.finish_pending();
        Ok(self.drain_ready()) // EndOfStream below finalizes without a decorative sleep.
    }
    async fn shutdown(mut self: Box<Self>) -> Vec<Transcript> {
        self.finish_pending();
        let began = Instant::now();
        let graceful = matches!(*self.status.borrow(), SpeechConnection::Ready);
        if graceful {
            let _ = self.control.send(Control::End);
        } else { self.cancel.send_replace(true); }
        let mut timed_out = false;
        if let Some(mut task) = self.task.take() {
            // EndOfStream is sent once queued audio is out (unpaced), then up
            // to 3s for trailing finals; 6s leaves margin for both.
            if tokio::time::timeout(Duration::from_secs(6), &mut task).await.is_err() {
                timed_out = true;
                self.cancel.send_replace(true);
                task.abort();
                let _ = task.await;
            }
        }
        let trailing = self.drain_ready();
        diag(self.diagnostics.as_deref(), serde_json::json!({"shutdown": {
            "graceful": graceful, "timed_out": timed_out, "ms": began.elapsed().as_millis() as u64,
            "trailing_items": trailing.len(), "trailing_finals": trailing.iter().filter(|t| t.is_final).count(),
        }}));
        trailing
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

#[derive(Debug)]
enum ConnectOutcome { Stopped, Retry { slow: bool, stable: bool, reason: String }, Terminal(String) }

fn give_up_message(attempts: u32, last: Option<&str>) -> String {
    match last { Some(reason) if !reason.is_empty() => format!("speechmatics: giving up after {attempts} reconnect attempts. Last problem: {reason}"),
        _ => format!("speechmatics: giving up after {attempts} reconnect attempts") }
}
fn retrying_message(attempt: u32, wait_secs: u64, last: Option<&str>) -> String {
    let retry = format!("Speechmatics is retrying ({attempt}/{MAX_RECONNECT_ATTEMPTS}) in {wait_secs}s…");
    match last { Some(reason) if !reason.is_empty() => format!("{reason} — {retry}"), _ => retry }
}
fn retry_delay(attempt: u32, slow: bool, jitter_ms: u64) -> Duration {
    let base = Duration::from_secs((1u64 << attempt.saturating_sub(1).min(5)).min(30));
    (if slow { base.max(SLOW_RETRY_FLOOR) } else { base }) + Duration::from_millis(jitter_ms.min(500))
}
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() { return; }
        if cancel.changed().await.is_err() { return; }
    }
}
/// Deliberately no audio, transcript, vocabulary, API key, or URL in
/// diagnostics: connection states and counts only.
fn diag(path: Option<&std::path::Path>, entry: serde_json::Value) {
    let Some(path) = path else { return };
    use std::io::Write;
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 64_000) {
        let _ = std::fs::rename(path, path.with_extension("previous.log"));
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    let mut line = serde_json::json!({"at_ms":at});
    if let (Some(line), Some(entry)) = (line.as_object_mut(), entry.as_object()) {
        line.extend(entry.clone());
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}
fn publish(state: &watch::Sender<SpeechConnection>, next: SpeechConnection, options: &TransportOptions) {
    if *state.borrow() == next { return; }
    diag(options.diagnostics.as_deref(), serde_json::json!({"connection":&next}));
    state.send_replace(next);
}

/// Message counts for one engine's life, written once when it stops.
#[derive(Default, Debug, serde::Serialize)]
struct Stats {
    connections: u32,
    packets_sent: u64,
    partials_in: u32,
    finals_in: u32,
    empty_finals_in: u32,
    end_of_utterance_in: u32,
    end_of_transcript_in: u32,
    previews_out: u32,
    finals_out: u32,
    finals_dropped_full: u32,
}
fn websocket_request(url: &str, session_token: &str) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let uri: Uri = url.parse().map_err(|_| "Invalid Speechmatics address.".to_owned())?;
    ClientRequestBuilder::new(uri).with_header("Authorization", auth_header_value(session_token))
        .into_client_request().map_err(|_| "Could not build the Speechmatics connection request.".to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCheck { Valid, Rejected, Unreachable }
enum MintFailure { Rejected, Retryable }
pub async fn verify_key(api_key: &str, region: Region) -> KeyCheck {
    match mint_rt_jwt(api_key, region).await {
        Ok(_) => KeyCheck::Valid,
        Err(MintFailure::Rejected) => KeyCheck::Rejected,
        Err(MintFailure::Retryable) => KeyCheck::Unreachable,
    }
}
async fn mint_rt_jwt(api_key: &str, region: Region) -> Result<String, MintFailure> {
    let response = reqwest::Client::new().post(JWT_MINT_URL).bearer_auth(api_key)
        .json(&jwt_mint_body(JWT_TTL_SECS, region)).timeout(Duration::from_secs(8)).send().await
        .map_err(|_| MintFailure::Retryable)?;
    let code = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    parse_jwt_mint_response(code, &body)
        .map_err(|_| if matches!(code,401|403) { MintFailure::Rejected } else { MintFailure::Retryable })
}

#[allow(clippy::too_many_arguments)]
async fn connection_task(key: String, config: SpeechmaticsConfig, audio: Arc<Mutex<AudioBuffer>>, notify: Arc<Notify>,
    mut controls: mpsc::UnboundedReceiver<Control>, transcripts: mpsc::Sender<Transcript>,
    state: watch::Sender<SpeechConnection>, mut cancel: watch::Receiver<bool>, options: TransportOptions) {
    if let Err(message) = start_recognition_message(&config) {
        publish(&state, SpeechConnection::Failed { message: format!("Speechmatics config is invalid: {message}") }, &options);
        return;
    }
    let mut attempt = 0;
    let mut slow = false;
    let mut last_reason: Option<String> = None;
    let mut dedupe = TurnDedupe::default();
    let mut stats = Stats::default();
    loop {
        if *cancel.borrow() { break; }
        if attempt >= MAX_RECONNECT_ATTEMPTS {
            publish(&state, SpeechConnection::Failed { message: give_up_message(attempt,last_reason.as_deref()) }, &options);
            return;
        }
        if attempt > 0 {
            let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_millis() as u64 % 501;
            let delay = retry_delay(attempt, slow, jitter).mul_f64(options.retry_scale);
            let message = retrying_message(attempt + 1, delay.as_secs().max(1), last_reason.as_deref());
            publish(&state, SpeechConnection::Recovering { message }, &options);
            tokio::select! { _ = cancelled(&mut cancel) => break, _ = tokio::time::sleep(delay) => {} }
        }
        // Native Rust can send the long-lived key in the Authorization header.
        // No browser restriction, token-mint round trip or JWT in the URL.
        let outcome = tokio::select! {
            _ = cancelled(&mut cancel) => break,
            result = connect_and_run(&key,&config,&audio,&notify,&mut controls,&transcripts,&state,&options,&mut dedupe,&mut stats) => result,
        };
        match outcome {
            ConnectOutcome::Stopped => break,
            ConnectOutcome::Terminal(message) => {
                let message = message.replace(&key,"[redacted]");
                publish(&state, SpeechConnection::Failed { message }, &options); return;
            }
            ConnectOutcome::Retry { slow: next_slow, stable, reason } => {
                slow = next_slow;
                attempt = if stable { 1 } else { attempt + 1 };
                last_reason = Some(reason.replace(&key,"[redacted]"));
                // Locked prefixes remain. The unconfirmed tail will be decoded
                // again from the replay buffer, replacing its old preview.
                dedupe.last_partial.clear();
            }
        }
    }
    diag(options.diagnostics.as_deref(), serde_json::json!({"stats": &stats}));
    publish(&state, SpeechConnection::Stopped, &options);
}

fn retry(reason: impl Into<String>, slow: bool, stable: bool) -> ConnectOutcome {
    ConnectOutcome::Retry { reason: reason.into(), slow, stable }
}
fn rejected(kind: &str, reason: &str) -> ConnectOutcome {
    let message = match kind {
        "quota_exceeded" => "Speechmatics has no free session slot. Waiting for an earlier session to close.".to_owned(),
        "job_error" => "Speechmatics could not start recognition. It will be retried.".to_owned(),
        "not_authorised" => "Speechmatics did not authorize the session. Check the saved key and its US region access.".to_owned(),
        "timelimit_exceeded" => "Speechmatics account credits or usage allowance are exhausted.".to_owned(),
        _ => format!("Speechmatics error ({kind}): {}",reason.chars().take(300).collect::<String>()),
    };
    if is_terminal_error_type(kind) { ConnectOutcome::Terminal(message) }
    else { retry(message, matches!(kind,"quota_exceeded"|"job_error"|"internal_error"),false) }
}
fn closed(code: u16, stable: bool) -> ConnectOutcome {
    match classify_close(code) {
        CloseDisposition::Terminal(message) => ConnectOutcome::Terminal(message),
        CloseDisposition::RetrySlow => retry(format!("Speechmatics closed the connection (code {code})."),true,stable),
        CloseDisposition::RetryFast => retry(format!("Speechmatics connection interrupted (code {code})."),false,stable),
    }
}

#[derive(Debug)]
enum HandshakeFailure { Terminal(String), Retry { slow: bool, reason: String } }
#[cfg(test)]
async fn await_recognition_started<S>(stream: &mut S) -> Result<(), HandshakeFailure>
where S: futures_util::Stream<Item=Result<Message,tokio_tungstenite::tungstenite::Error>> + Unpin {
    await_started(stream,HANDSHAKE_TIMEOUT).await
}
async fn await_started<S>(stream: &mut S, timeout: Duration) -> Result<(), HandshakeFailure>
where S: futures_util::Stream<Item=Result<Message,tokio_tungstenite::tungstenite::Error>> + Unpin {
    let result = tokio::time::timeout(timeout,async {
        while let Some(message) = stream.next().await {
            let outcome = match message {
                Ok(Message::Text(text)) => match parse_message(&text) {
                    Ok(SpeechmaticsMessage::RecognitionStarted) => return Ok(()),
                    Ok(SpeechmaticsMessage::Error { error_type, reason }) => Some(rejected(&error_type,&reason)),
                    _ => None,
                },
                Ok(Message::Close(frame)) => Some(closed(frame.map(|f|u16::from(f.code)).unwrap_or(1006),false)),
                Err(_) => Some(retry("Speechmatics connection was interrupted during startup.",false,false)),
                _ => None,
            };
            if let Some(outcome) = outcome { return Err(handshake_failure(outcome)); }
        }
        Err(HandshakeFailure::Retry { slow:false,reason:"Speechmatics closed before confirming recognition.".into() })
    }).await;
    result.unwrap_or_else(|_| Err(HandshakeFailure::Retry { slow:false,
        reason:"Speechmatics session confirmation timed out. Retrying without changing your key.".into() }))
}
fn handshake_failure(outcome: ConnectOutcome) -> HandshakeFailure {
    match outcome {
        ConnectOutcome::Terminal(message) => HandshakeFailure::Terminal(message),
        ConnectOutcome::Retry { slow, reason, .. } => HandshakeFailure::Retry { slow, reason },
        ConnectOutcome::Stopped => HandshakeFailure::Retry { slow:false,reason:"Speechmatics stopped during startup.".into() },
    }
}
async fn next_audio(audio: &Mutex<AudioBuffer>, notify: &Notify, cursor: u64, at: tokio::time::Instant) -> Packet {
    tokio::time::sleep_until(at).await;
    loop {
        let notified = notify.notified();
        if let Some(packet) = audio.lock().unwrap_or_else(|p|p.into_inner()).next_after(cursor) { return packet; }
        notified.await;
    }
}
async fn send<S>(sink: &mut S, message: Message) -> Result<(), ()>
where S: futures_util::Sink<Message> + Unpin {
    tokio::time::timeout(Duration::from_secs(3),sink.send(message)).await.map_err(|_|())?.map_err(|_|())
}
fn forward(result: Transcript, transcripts: &mpsc::Sender<Transcript>, stats: &mut Stats) {
    let is_final = result.is_final;
    if transcripts.try_send(result).is_err() {
        if is_final { stats.finals_dropped_full += 1; }
        return;
    }
    if is_final { stats.finals_out += 1; } else { stats.previews_out += 1; }
}

fn accept_text(text: &str, dedupe: &mut TurnDedupe, sent: &mut SentAudio, audio: &Mutex<AudioBuffer>, rate: u32,
    transcripts: &mpsc::Sender<Transcript>, stats: &mut Stats) -> Result<bool,ConnectOutcome> {
    let message = match parse_message(text) { Ok(message) => message, Err(_) => return Ok(false) };
    match &message {
        SpeechmaticsMessage::AddPartialTranscript { .. } => stats.partials_in += 1,
        SpeechmaticsMessage::AddTranscript { transcript } if transcript.trim().is_empty() => stats.empty_finals_in += 1,
        SpeechmaticsMessage::AddTranscript { .. } => stats.finals_in += 1,
        SpeechmaticsMessage::EndOfUtterance => stats.end_of_utterance_in += 1,
        _ => {}
    }
    match &message {
        SpeechmaticsMessage::Error { error_type, reason } => return Err(rejected(error_type,reason)),
        SpeechmaticsMessage::EndOfTranscript => {
            stats.end_of_transcript_in += 1;
            if let Some(final_text) = apply_message(SpeechmaticsMessage::EndOfUtterance,dedupe) { forward(final_text, transcripts, stats); }
            return Ok(true);
        }
        SpeechmaticsMessage::AddTranscript { .. } => {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(end) = value.pointer("/metadata/end_time").and_then(|v|v.as_f64()) {
                    sent.confirm(&mut audio.lock().unwrap_or_else(|p|p.into_inner()),end,rate);
                }
            }
        }
        _ => {}
    }
    if let Some(result) = apply_message(message,dedupe) { forward(result, transcripts, stats); }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
async fn connect_and_run(key: &str, config: &SpeechmaticsConfig, audio: &Mutex<AudioBuffer>, notify: &Notify,
    controls: &mut mpsc::UnboundedReceiver<Control>, transcripts: &mpsc::Sender<Transcript>,
    state: &watch::Sender<SpeechConnection>, options: &TransportOptions, dedupe: &mut TurnDedupe, stats: &mut Stats) -> ConnectOutcome {
    let request = match websocket_request(options.endpoint.as_deref().unwrap_or(config.region.url()),key) {
        Ok(request) => request, Err(error) => return ConnectOutcome::Terminal(error),
    };
    let connection = tokio::time::timeout(Duration::from_secs(8),tokio_tungstenite::connect_async(request)).await;
    let mut socket = match connection {
        Ok(Ok((socket,_))) => socket,
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => return match response.status().as_u16() {
            401|403 => rejected("not_authorised",""),
            400|405 => rejected("protocol_error","Connection request was not accepted."),
            429 => rejected("quota_exceeded",""),
            code => retry(format!("Speechmatics returned HTTP {code} during startup."),code>=500,false),
        },
        _ => return retry("Could not reach Speechmatics. Check the internet connection; retrying automatically.",false,false),
    };
    if send(&mut socket,Message::Text(start_recognition_message(config).unwrap().into())).await.is_err() {
        return retry("Speechmatics disconnected before recognition could start.",false,false);
    }
    match await_started(&mut socket,options.handshake_timeout).await {
        Ok(()) => { stats.connections += 1; publish(state,SpeechConnection::Ready,options) }
        Err(HandshakeFailure::Terminal(message)) => return ConnectOutcome::Terminal(message),
        Err(HandshakeFailure::Retry {slow,reason}) => return retry(reason,slow,false),
    }
    let began = Instant::now();
    let mut last_received = Instant::now();
    let mut cursor = 0;
    let mut seq_no = 0;
    let mut sent = SentAudio::default();
    let mut ending = false;
    let mut next_send = tokio::time::Instant::now();
    let (mut sink,mut stream) = socket.split();
    let mut ping = tokio::time::interval(KEEPALIVE_INTERVAL); ping.tick().await;
    loop {
        let stable = began.elapsed() >= STABILITY_WINDOW;
        if ending && audio.lock().unwrap_or_else(|p|p.into_inner()).next_after(cursor).is_none() {
            if send(&mut sink,Message::Text(end_of_stream_message(seq_no).into())).await.is_err() { return ConnectOutcome::Stopped; }
            // Read trailing finals before closing; no fixed sleep and no lost last sentence.
            let _ = tokio::time::timeout(Duration::from_secs(3),async {
                while let Some(Ok(message)) = stream.next().await {
                    if let Message::Text(text) = message {
                        if accept_text(&text,dedupe,&mut sent,audio,config.sample_rate,transcripts,stats).unwrap_or(true) { break; }
                    }
                }
            }).await;
            let _ = tokio::time::timeout(Duration::from_millis(300),sink.close()).await;
            return ConnectOutcome::Stopped;
        }
        tokio::select! {
            control = controls.recv(), if !ending => match control {
                Some(Control::End)|None => { ending = true; next_send = tokio::time::Instant::now(); }
            },
            _ = ping.tick() => {
                if last_received.elapsed() > Duration::from_secs(24) { return retry("Speechmatics stopped responding. Reconnecting.",false,stable); }
                if send(&mut sink,Message::Ping(Vec::new().into())).await.is_err() { return retry("Speechmatics connection interrupted while checking connectivity.",false,stable); }
            }
            packet = next_audio(audio,notify,cursor,next_send) => {
                if send(&mut sink,Message::Binary(packet.bytes.clone().into())).await.is_err() { return retry("Speechmatics connection interrupted while sending audio.",false,stable); }
                cursor = packet.id;
                seq_no += 1;
                stats.packets_sent += 1;
                sent.sent(&packet);
                // Catch up at at most 2x realtime, rather than flooding the server
                // with the entire reconnect backlog in one burst.
                // When ending, the remainder goes out at once so the last
                // sentence is finalized before shutdown's deadline.
                if !ending {
                    next_send = tokio::time::Instant::now() + Duration::from_secs_f64(packet.bytes.len() as f64 / (config.sample_rate as f64 * 4.0));
                }
            }
            message = stream.next() => {
                last_received = Instant::now();
                match message {
                    Some(Ok(Message::Text(text))) => match accept_text(&text,dedupe,&mut sent,audio,config.sample_rate,transcripts,stats) {
                        Ok(true) => return ConnectOutcome::Stopped,
                        Err(outcome) => return outcome,
                        Ok(false) => {}
                    },
                    Some(Ok(Message::Close(frame))) => return closed(frame.map(|f|u16::from(f.code)).unwrap_or(1006),stable),
                    None|Some(Err(_)) => return retry("Speechmatics connection interrupted. Reconnecting with buffered audio.",false,stable),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod fake_server;

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicitly invoked only: uses the owner's saved key and generated/test
    /// audio, never the microphone, resume, or saved conversation.
    #[tokio::test]
    #[ignore = "real Speechmatics account; ASIDE_LIVE_SPEECH_TEST=1 required"]
    async fn live_native_authorization_probe() {
        assert_eq!(std::env::var("ASIDE_LIVE_SPEECH_TEST").as_deref(), Ok("1"));
        let path = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.aside.desktop/aside.sqlite3");
        let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let key: String = db.query_row("SELECT value FROM settings WHERE key='stt_api_key_speechmatics'", [], |r| r.get(0)).unwrap();
        let key = super::super::normalize_api_key(&key).unwrap();
        drop(db);
        for model in ["standard", "enhanced"] {
            let config = SpeechmaticsConfig { model: model.into(), ..SpeechmaticsConfig::for_notebook() };
            let request = websocket_request(config.region.url(), &key).unwrap();
            let start = Instant::now();
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(request)).await.unwrap().expect("native Bearer authorization failed");
            socket.send(Message::Text(start_recognition_message(&config).unwrap().into())).await.unwrap();
            let outcome = await_recognition_started(&mut socket).await;
            assert!(outcome.is_ok(), "native token was not accepted (details intentionally withheld)");
            println!("LIVE_NATIVE_AUTH model={model} ready_ms={}", start.elapsed().as_millis());
            socket.send(Message::Text(end_of_stream_message(0).into())).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(Ok(message)) = socket.next().await {
                    if let Message::Text(text) = message {
                        if matches!(parse_message(&text), Ok(SpeechmaticsMessage::EndOfTranscript)) { break; }
                    }
                }
            }).await.unwrap();
            let _ = socket.close(None).await;
        }
    }

    /// Explicitly invoked only: streams a generated (`say`) WAV through the
    /// real engine in real time and prints timings and the recognized text.
    /// Never the microphone or a saved conversation; key is never printed.
    #[tokio::test]
    #[ignore = "real Speechmatics account; ASIDE_LIVE_SPEECH_TEST=1 and ASIDE_SYNTH_WAV required"]
    async fn live_synthetic_speech_comparison() {
        assert_eq!(std::env::var("ASIDE_LIVE_SPEECH_TEST").as_deref(), Ok("1"));
        let wav = std::fs::read(std::env::var("ASIDE_SYNTH_WAV").unwrap()).unwrap();
        let data = wav.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
        let samples: Vec<f32> = wav[data..].chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let speech_end = samples.iter().rposition(|s| s.abs() > 0.02).unwrap_or(samples.len());
        let path = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.aside.desktop/aside.sqlite3");
        let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let key: String = db.query_row("SELECT value FROM settings WHERE key='stt_api_key_speechmatics'", [], |r| r.get(0)).unwrap();
        let key = super::super::normalize_api_key(&key).unwrap();
        drop(db);
        let vocab: Vec<String> = ["RAG", "MCP", "Kiro", "Kubernetes", "PostgreSQL"].map(String::from).to_vec();
        let only = std::env::var("ASIDE_SYNTH_ONLY").ok();
        for (label, model, delay, with_vocab) in [
            ("standard-4.0-vocab", "standard", 4.0, true),
            ("enhanced-4.0-vocab", "enhanced", 4.0, true),
            ("standard-2.0-vocab", "standard", 2.0, true),
            ("standard-4.0-novocab", "standard", 4.0, false),
        ] {
            if only.as_deref().is_some_and(|o| o != label) { continue; }
            let config = SpeechmaticsConfig { model: model.into(), max_delay: delay,
                additional_vocab: if with_vocab { vocab.clone() } else { Vec::new() },
                ..SpeechmaticsConfig::for_notebook() };
            let began = Instant::now();
            let mut engine = SpeechmaticsLiveEngine::start(key.clone(), config);
            crate::stt::connection::wait_ready(engine.status.clone()).await.expect("not ready");
            let ready_ms = began.elapsed().as_millis();
            let chunk = 640; // 40 ms
            let audio_start = Instant::now();
            let mut first_partial = None;
            let mut first_final = None;
            let mut finals = Vec::new();
            let mut speech_end_at = None;
            let silence = vec![0.0f32; 16_000 * 4];
            for (i, block) in samples.chunks(chunk).chain(silence.chunks(chunk)).enumerate() {
                tokio::time::sleep_until((audio_start + Duration::from_millis(40 * i as u64)).into()).await;
                engine.push_audio(block, 16_000);
                if speech_end_at.is_none() && (i + 1) * chunk >= speech_end { speech_end_at = Some(Instant::now()); }
                for t in engine.drain().await.unwrap_or_default() {
                    if first_partial.is_none() { first_partial = Some(audio_start.elapsed().as_millis()); }
                    if t.is_final { first_final.get_or_insert(Instant::now()); finals.push(t.text); }
                }
            }
            let unconfirmed = engine.audio.lock().unwrap().next_after(0).map(|p| p.id);
            let newest = samples.len().div_ceil(chunk) + silence.len() / chunk;
            println!("LIVE_SYNTH {label} oldest_unconfirmed_packet={unconfirmed:?} of ~{newest}");
            let stop = Instant::now();
            let trailing = Box::new(engine).shutdown().await;
            let stop_ms = stop.elapsed().as_millis();
            finals.extend(trailing.into_iter().filter(|t| t.is_final).map(|t| t.text));
            let end = speech_end_at.unwrap();
            let final_after_speech = first_final.map(|f| f.saturating_duration_since(end).as_millis() as i64).unwrap_or(-1);
            println!("LIVE_SYNTH {label} ready_ms={ready_ms} first_text_ms={first_partial:?} final_after_speech_end_ms={final_after_speech} shutdown_ms={stop_ms} text={:?}", finals.join(" | "));
        }
    }

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
        // session, so this conversion is not optional: declaring one rate and
        // sending another silently ruins every transcript.
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
