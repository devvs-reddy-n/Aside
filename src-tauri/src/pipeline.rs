//! Wires continuous audio capture into a running speech-to-text engine.
//!
//! This is the integration layer, deliberately its own top-level module
//! rather than living inside `audio` or `stt`: `audio` only ever produces
//! signals (level, VAD, and now `StreamMsg` — see `audio/channel.rs`) with
//! zero knowledge of transcription, and `stt` only ever turns audio into
//! text with zero knowledge of where that audio came from. This module is
//! what connects them, matching the Phase 4 continuation research's
//! recommended shape (`docs/evidence.md`).
//!
//! ## Data flow
//!
//! ```text
//! [cpal audio callback thread]           (audio/mic.rs, audio/system.rs)
//!     ChannelPipeline::process_f32/i16 -> StreamRoute (std::sync::mpsc, non-blocking send)
//!
//! [bridge OS thread, one per active channel]
//!     std::sync::mpsc::Receiver::recv() (blocking, on its own thread) -> tokio::sync::mpsc (async)
//!
//! [tokio task, one per active channel, owns Box<dyn SttEngine>]
//!     tokio::select! on: a periodic drain tick, or the next StreamMsg
//!     -> engine.push_audio() / engine.drain() / engine.flush()
//!     -> persist_finals() -> storage/transcripts.rs (only if a session_id was given, only `is_final` ones)
//!     -> AppHandle::emit("mic-transcript" | "system-transcript", TranscriptEvent)
//! ```
//!
//! **Persistence (Phase 5.1/5.2).** `start()` optionally takes a
//! `session_id`. When one is given, every final transcript is written to
//! `storage/transcripts.rs` before it's emitted to the frontend — see
//! `persist_finals`'s doc comment for the exact rule (finals only) and an
//! honest note about what its `start_ms`/`end_ms` actually measure (receipt
//! time, not true audio-sample timestamps, since no engine's `Transcript`
//! carries those). With no `session_id`, this module behaves exactly as it
//! did before this landed: live events only, nothing durable.
//!
//! **Why a bridge thread, not just an async channel from the audio
//! callback.** The audio callback is a real-time thread that must never
//! block — `std::sync::mpsc::Sender::send` on an unbounded channel is the
//! safe non-blocking primitive for that side. But
//! `std::sync::mpsc::Receiver::recv` *blocks its OS thread*, which must
//! never be a tokio worker thread (it would stall every other task sharing
//! that thread). The bridge thread absorbs that blocking call and re-emits
//! each message into a `tokio::sync::mpsc` channel the async task can
//! `.await` on alongside a timer in one `select!`.
//!
//! **No VAD flush.** Every live engine has its own stop-of-speech: Parakeet
//! EOU's `[EOU]` token, Deepgram `UtteranceEnd` / `endpointing`, Speechmatics
//! `max_delay` / `AddTranscript`. Speaking-light VAD stays UI-only (the
//! original Phase 2 rule). `flush()` runs only when this task stops, so a
//! trailing open utterance is still committed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as sync_mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc as tokio_mpsc, watch};
use uuid::Uuid;

use crate::audio::channel::{Channel, StreamMsg, StreamRoute};
use crate::storage::{transcripts as transcript_storage, Db};
use crate::stt::{SttEngine, Transcript};
use crate::transcript_coalesce::{merge_notebook_text, should_continue_line};

/// Backup `drain()` cadence for results that arrive without a new audio
/// buffer (cloud WebSocket text after a pause). Primary drain is on every
/// audio message — 1.5s was the old TDT re-inference tick and made true
/// streaming engines feel a second late; 100ms is the agreed backup.
const DRAIN_TICK_INTERVAL: Duration = Duration::from_millis(100);

/// Holds the routing cells `audio/mic.rs`/`audio/system.rs`'s callbacks
/// check every buffer, plus which channels currently have a running
/// transcription task. Managed as Tauri state, one instance for the app.
#[derive(Default)]
pub struct TranscriptionRouting {
    pub mic: StreamRoute,
    pub system: StreamRoute,
    running: Mutex<RunningState>,
    /// One start at a time per channel, so a double click cannot build two
    /// cloud sessions that compete for the same Speechmatics slot.
    mic_start: tokio::sync::Mutex<()>,
    system_start: tokio::sync::Mutex<()>,
}

impl TranscriptionRouting {
    pub fn start_lock(&self, channel: Channel) -> &tokio::sync::Mutex<()> {
        match channel {
            Channel::Mic => &self.mic_start,
            Channel::System => &self.system_start,
        }
    }
}

#[derive(Default)]
struct RunningState {
    mic: ChannelTask,
    system: ChannelTask,
}

/// `active` is the running task's stop flag. `finishing` tells when the most
/// recently stopped task has flushed and closed its engine.
#[derive(Default)]
struct ChannelTask {
    active: Option<Arc<AtomicBool>>,
    done: Option<watch::Receiver<bool>>,
    finishing: Vec<watch::Receiver<bool>>,
}

/// One transcript segment emitted to the frontend over `mic-transcript` /
/// `system-transcript`. A separate type from `stt::Transcript` rather than
/// reusing it directly — this is a wire contract with the frontend, and
/// keeping it distinct means `stt::Transcript` can evolve (e.g. gain
/// engine-internal fields later) without silently changing what the UI
/// receives.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEvent {
    pub text: String,
    pub is_final: bool,
}

fn event_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Mic => "mic-transcript",
        Channel::System => "system-transcript",
    }
}

/// A separate event name from `event_name`'s transcript stream, mirroring how
/// `audio/mic.rs`/`audio/system.rs` already split `*-level` from `*-error`.
/// Without this, an engine failure (bad API key, exhausted Deepgram
/// reconnects) was only ever `eprintln!`'d — invisible to anyone not
/// watching the terminal, and indistinguishable in the UI from "no speech
/// yet" (found by adversarial review; see `docs/evidence.md`).
fn error_event_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Mic => "mic-transcript-error",
        Channel::System => "system-transcript-error",
    }
}

fn route_for(routing: &TranscriptionRouting, channel: Channel) -> &StreamRoute {
    match channel {
        Channel::Mic => &routing.mic,
        Channel::System => &routing.system,
    }
}

/// Starts continuously transcribing `channel`'s captured audio with
/// `engine`. If a transcription task is already running for this channel
/// (an engine switch mid-session, or a duplicate start), it's stopped first
/// — never two engines racing to consume the same audio.
///
/// Does **not** require capture to already be running: the routing cell is
/// just a cell either side can populate first. If capture starts later, the
/// very next audio callback picks up the now-present sender.
///
/// `session_id` and `origin` are Phase 5.2's persistence plumbing: every
/// **final** transcript this task produces is written to
/// `storage/transcripts.rs`, tagged with `channel` and `origin` and
/// associated with `session_id` (see `persist_finals`). `session_id: None`
/// (the caller has no active session — nothing forces one to exist today,
/// see `docs/decisions.md`) means transcription still runs and still emits
/// live events exactly as before; it just writes nothing to disk. `origin`
/// is `&'static str` rather than `EngineKind` to keep this module from
/// depending on `stt::EngineKind` for a value it only ever stringifies —
/// callers pass `EngineKind::origin_db_value()`.
/// A channel's audio route, installed and already accepting real captured
/// audio before its engine necessarily exists — see [`prepare_route`].
pub struct PreparedRoute {
    channel: Channel,
    tokio_rx: tokio_mpsc::UnboundedReceiver<StreamMsg>,
    stop_flag: Arc<AtomicBool>,
    done: watch::Sender<bool>,
}

/// Installs `channel`'s audio route immediately and returns a handle to
/// finish starting once an engine is ready. Split out from `start` so
/// `commands/stt.rs::start_channel_transcription` can call this **before**
/// constructing its (sometimes slow — a first-ever local Parakeet load
/// measured at ~400ms for real ONNX weight loading plus session init, see
/// `stt::parakeet_local`'s `measures_the_real_click_to_route_live_window`
/// test) engine, not after.
///
/// Before this split, `start` both installed the route *and* required an
/// already-built `engine` in the same call — so any real mic/system audio
/// captured in the window between clicking "Transcribe" and that engine
/// build finishing had nowhere to go: `audio/channel.rs::forward_to_stt`
/// only sends when a route's sender is already present, and there is no
/// buffer upstream of it. That audio wasn't delayed, it was silently and
/// permanently discarded — a real, if usually small, source of "the first
/// few words are gone" reports. Calling `prepare_route` first closes that
/// gap structurally rather than just shrinking it: the route (and the
/// unbounded channel behind it) exists from the first instant, so any audio
/// captured while the engine is still loading simply queues — exactly like
/// this module's already-proven backlog/catch-up behavior for a real
/// mid-session pause — and `run()` drains it the moment it starts, instead
/// of that audio never having existed as far as the pipeline is concerned.
pub fn prepare_route(routing: &TranscriptionRouting, channel: Channel) -> PreparedRoute {
    stop(routing, channel);

    let (std_tx, std_rx) = sync_mpsc::channel::<StreamMsg>();
    let (tokio_tx, tokio_rx) = tokio_mpsc::unbounded_channel::<StreamMsg>();

    // A dedicated OS thread, not `spawn_blocking` — that pool exists for
    // short-lived blocking calls, not a thread parked in a blocking `recv`
    // loop for a whole session's duration.
    std::thread::spawn(move || {
        while let Ok(msg) = std_rx.recv() {
            if tokio_tx.send(msg).is_err() {
                break; // the async side stopped listening
            }
        }
    });

    *route_for(routing, channel)
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(std_tx);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let (done, done_rx) = watch::channel(false);
    {
        let mut running = routing.running.lock().unwrap_or_else(|p| p.into_inner());
        let slot = task_slot(&mut running, channel);
        slot.active = Some(stop_flag.clone());
        slot.done = Some(done_rx);
    }

    PreparedRoute {
        channel,
        tokio_rx,
        stop_flag,
        done,
    }
}

/// Finishes starting transcription once `engine` is ready, using a route
/// already installed by [`prepare_route`]. Any audio queued during the gap
/// between that call and this one is still sitting in `prepared`'s channel
/// and is the very first thing `run()`'s loop drains.
#[allow(clippy::too_many_arguments)]
pub fn start_with_prepared_route(
    app: AppHandle,
    prepared: PreparedRoute,
    engine: Box<dyn SttEngine>,
    session_id: Option<Uuid>,
    origin: &'static str,
) {
    tauri::async_runtime::spawn(run(
        app,
        prepared.channel,
        engine,
        prepared.tokio_rx,
        prepared.stop_flag,
        session_id,
        origin,
        prepared.done,
    ));
}

fn task_slot(running: &mut RunningState, channel: Channel) -> &mut ChannelTask {
    match channel {
        Channel::Mic => &mut running.mic,
        Channel::System => &mut running.system,
    }
}

/// Stops `channel` and waits until its old engine has sent its last words and
/// closed. A new cloud session must not overlap the old one: Speechmatics
/// counts both against the account's concurrent-session limit.
pub async fn stop_and_wait(routing: &TranscriptionRouting, channel: Channel) {
    stop(routing, channel);
    let waiting = {
        let mut running = routing.running.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut task_slot(&mut running, channel).finishing)
    };
    for mut done in waiting {
        // Bounded: a stuck network close must not freeze the mic button.
        let _ = tokio::time::timeout(Duration::from_secs(8), done.wait_for(|d| *d)).await;
    }
}

/// Stops `channel`'s transcription task if one is running. A no-op, not an
/// error, if none is — matches `stop_mic_capture`'s own idempotent shape.
pub fn stop(routing: &TranscriptionRouting, channel: Channel) {
    *route_for(routing, channel)
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = None;

    let mut running = routing.running.lock().unwrap_or_else(|p| p.into_inner());
    let slot = task_slot(&mut running, channel);
    if let Some(flag) = slot.active.take() {
        flag.store(true, Ordering::SeqCst);
    }
    if let Some(done) = slot.done.take() {
        slot.finishing.retain(|d| !*d.borrow());
        slot.finishing.push(done);
    }
}

/// True if a transcription task is currently running for `channel`.
pub fn is_running(routing: &TranscriptionRouting, channel: Channel) -> bool {
    let running = routing.running.lock().unwrap_or_else(|p| p.into_inner());
    match channel {
        Channel::Mic => running.mic.active.is_some(),
        Channel::System => running.system.active.is_some(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    app: AppHandle,
    channel: Channel,
    mut engine: Box<dyn SttEngine>,
    mut rx: tokio_mpsc::UnboundedReceiver<StreamMsg>,
    stop: Arc<AtomicBool>,
    session_id: Option<Uuid>,
    origin: &'static str,
    done: watch::Sender<bool>,
) {
    let mut connection = engine.connection_status();
    let mut was_recovering = false;
    let mut tick = tokio::time::interval(DRAIN_TICK_INTERVAL);
    // The default. Stated explicitly since it's what makes a fixed-interval
    // `tokio::time::interval` behave like the "self-rescheduling, don't pile
    // ticks up against a still-running pass" loop the research called for:
    // a tick that runs long delays the *next* tick by that same amount
    // rather than firing a burst of catch-up ticks once the slow one ends.
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Tracks the last error string emitted to the frontend so a permanently
    // failed engine (e.g. Deepgram after exhausting reconnects) doesn't spam
    // an identical event every 1.5s tick for the rest of the session.
    let mut last_reported_error: Option<String> = None;
    let mut segment_start_ms = now_ms();
    let mut utterance_started_ms: Option<u64> = None;

    loop {
        if stop.load(Ordering::SeqCst) {
            while let Ok(msg) = rx.try_recv() {
                if let StreamMsg::Audio {
                    samples,
                    sample_rate,
                } = msg
                {
                    engine.push_audio(&samples, sample_rate);
                }
            }
            match engine.drain().await {
                Ok(transcripts) => {
                    persist_and_emit(
                        &app,
                        session_id,
                        channel,
                        origin,
                        transcripts,
                        &mut segment_start_ms,
                        &mut utterance_started_ms,
                    );
                }
                Err(e) => report_error(&app, channel, e, &mut last_reported_error),
            }
            break;
        }
        tokio::select! {
            _ = tick.tick() => {
                if let Some(status) = connection.as_mut() {
                    if status.has_changed().unwrap_or(false) {
                        use crate::stt::connection::SpeechConnection;
                        // Only a recovery is announced; the first Ready is the
                        // mic button turning on.
                        match status.borrow_and_update().clone() {
                            SpeechConnection::Recovering { .. } => was_recovering = true,
                            SpeechConnection::Ready if was_recovering => {
                                was_recovering = false;
                                last_reported_error = None;
                                let _ = app.emit(error_event_name(channel), "Speech recognition reconnected.".to_string());
                            }
                            _ => {}
                        }
                    }
                }
                match engine.drain().await {
                    Ok(transcripts) => {
                        persist_and_emit(
                            &app,
                            session_id,
                            channel,
                            origin,
                            transcripts,
                            &mut segment_start_ms,
                            &mut utterance_started_ms,
                        );
                    }
                    Err(e) => report_error(&app, channel, e, &mut last_reported_error),
                }
            }
            msg = rx.recv() => {
                match msg {
                    Some(StreamMsg::Audio { samples, sample_rate }) => {
                        engine.push_audio(&samples, sample_rate);
                        match engine.drain().await {
                            Ok(transcripts) => {
                                persist_and_emit(
                                    &app,
                                    session_id,
                                    channel,
                                    origin,
                                    transcripts,
                                    &mut segment_start_ms,
                                    &mut utterance_started_ms,
                                );
                            }
                            Err(e) => report_error(&app, channel, e, &mut last_reported_error),
                        }
                    }
                    Some(StreamMsg::SpeechEnd) => {}
                    None => break, // sender side gone — capture stopped without an explicit stop()
                }
            }
        }
    }

    // Commit whatever the engine still has open. Cloud engines treat this as
    // a soft finalize; local EOU pads silence so a trailing [EOU] can fire.
    match engine.flush().await {
        Ok(transcripts) => {
            persist_and_emit(
                &app,
                session_id,
                channel,
                origin,
                transcripts,
                &mut segment_start_ms,
                &mut utterance_started_ms,
            );
        }
        Err(e) => report_error(&app, channel, e, &mut last_reported_error),
    }
    // A cloud engine's last sentence arrives only after it is told the audio
    // has ended, so shutdown's words are saved like any other final.
    let trailing = engine.shutdown().await;
    persist_and_emit(
        &app,
        session_id,
        channel,
        origin,
        trailing,
        &mut segment_start_ms,
        &mut utterance_started_ms,
    );
    let _ = done.send(true);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The `channel` column value this pipeline writes for `channel` —
/// `Channel`'s own serde spelling (`#[serde(rename_all = "lowercase")]`)
/// already matches `storage/migrations.rs`'s `CHECK (channel IN
/// ('mic','system'))` exactly, so this just names that fact rather than
/// introducing a second mapping that could drift from it.
fn db_channel_str(channel: Channel) -> &'static str {
    match channel {
        Channel::Mic => "mic",
        Channel::System => "system",
    }
}

fn drop_speaker_echo(
    app: &AppHandle,
    channel: Channel,
    transcripts: Vec<Transcript>,
) -> Vec<Transcript> {
    if channel != Channel::Mic {
        return transcripts;
    }
    let them =
        crate::transcript_log::recent_them_turns(app, crate::transcript_coalesce::ECHO_LOOKBACK);
    if them.is_empty() {
        return transcripts;
    }
    transcripts
        .into_iter()
        .filter(|t| !crate::transcript_coalesce::is_likely_speaker_echo_of_any(&t.text, &them))
        .collect()
}

/// Others just finalized. If Me already landed as a copy of that line, take
/// it back from the in-memory log and (when a session exists) from SQLite.
fn forget_leading_mic_echo(app: &AppHandle, session_id: Option<Uuid>, transcripts: &[Transcript]) {
    let refs: Vec<String> = transcripts
        .iter()
        .filter(|t| t.is_final && !t.text.trim().is_empty())
        .map(|t| t.text.clone())
        .collect();
    if refs.is_empty() {
        return;
    }
    let Some(you) = crate::transcript_log::last_you_turn(app) else {
        return;
    };
    if !crate::transcript_coalesce::is_likely_speaker_echo_of_any(&you, &refs) {
        return;
    }
    let _ = crate::transcript_log::pop_last_you(app);
    let Some(session_id) = session_id else {
        return;
    };
    let db = app.state::<Db>();
    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
    if let Ok(Some(row)) = crate::storage::transcripts::last_for_channel(&conn, session_id, "mic") {
        if crate::transcript_coalesce::is_likely_speaker_echo_of_any(&row.text, &refs) {
            let _ = crate::storage::transcripts::delete_by_id(&conn, row.id);
        }
    }
}

/// Persists every **final** transcript in `transcripts` to `storage/transcripts.rs`,
/// tagged with `channel`/`origin` and associated with `session_id`. Interim
/// (`is_final: false`) entries are silently skipped — this is where
/// `docs/decisions.md`'s "Final transcripts are complete utterances, never
/// deltas" rule actually gets enforced for the persisted path; the storage
/// layer itself (`storage/transcripts.rs::insert_segment`) has no opinion
/// and would happily store either. A no-op, not an error, when `session_id`
/// is `None` — there is no session row to satisfy the table's foreign key
/// without one, and this pipeline is allowed to run with no active session
/// (live events still work; nothing survives a restart).
///
/// **Timing is a wall-clock approximation, stated plainly rather than
/// invented as something more precise than it is.** `stt::Transcript` (every
/// engine's output type) carries only `text` and `is_final` — none of the
/// three engines expose true per-utterance audio timestamps through it
/// today. So `start_ms`/`end_ms` (both `NOT NULL` in the schema) are backed
/// by receipt time instead: `end_ms` is "when this final was handed to the
/// pipeline", and `start_ms` is "when the previous final for this channel's
/// task ended" (or when the task itself started listening, for the very
/// first segment). This is honest about *when a segment was heard as done*,
/// not about the underlying audio's actual sample-accurate boundaries.
#[allow(clippy::too_many_arguments)]
fn persist_and_emit(
    app: &AppHandle,
    session_id: Option<Uuid>,
    channel: Channel,
    origin: &str,
    transcripts: Vec<Transcript>,
    segment_start_ms: &mut u64,
    utterance_started_ms: &mut Option<u64>,
) {
    let them =
        crate::transcript_log::recent_them_turns(app, crate::transcript_coalesce::ECHO_LOOKBACK);
    let dropped_echo_final = channel == Channel::Mic
        && transcripts.iter().any(|t| {
            t.is_final && crate::transcript_coalesce::is_likely_speaker_echo_of_any(&t.text, &them)
        });
    if channel == Channel::System {
        forget_leading_mic_echo(app, session_id, &transcripts);
    }
    let transcripts = drop_speaker_echo(app, channel, transcripts);
    persist_finals(
        app,
        session_id,
        channel,
        origin,
        &transcripts,
        segment_start_ms,
        utterance_started_ms,
    );
    if dropped_echo_final && transcripts.iter().all(|t| !t.is_final) {
        *utterance_started_ms = None;
    }
    emit_all(app, channel, session_id, transcripts);
}

fn persist_finals(
    app: &AppHandle,
    session_id: Option<Uuid>,
    channel: Channel,
    origin: &str,
    transcripts: &[Transcript],
    segment_start_ms: &mut u64,
    utterance_started_ms: &mut Option<u64>,
) {
    if session_id.is_none() {
        return;
    }
    let now = now_ms();
    if utterance_started_ms.is_none() && transcripts.iter().any(|t| !t.text.trim().is_empty()) {
        *utterance_started_ms = Some(now);
    }
    let speech_started_ms = utterance_started_ms.unwrap_or(now);
    let db = app.state::<Db>();
    persist_finals_to_db_at(
        &db,
        session_id,
        channel,
        origin,
        transcripts,
        segment_start_ms,
        now,
        speech_started_ms,
    );
    if transcripts.iter().any(|t| t.is_final) {
        *utterance_started_ms = None;
    }
}

/// The actual persistence logic, factored out from `persist_finals` so it's
/// testable against a real (in-memory) `Db` without needing a running Tauri
/// `AppHandle` — `persist_finals` above is just this plus the
/// `app.state::<Db>()` lookup `run()`'s call sites need.
#[cfg(test)]
fn persist_finals_to_db(
    db: &Db,
    session_id: Option<Uuid>,
    channel: Channel,
    origin: &str,
    transcripts: &[Transcript],
    segment_start_ms: &mut u64,
    now_ms: u64,
) {
    persist_finals_to_db_at(
        db,
        session_id,
        channel,
        origin,
        transcripts,
        segment_start_ms,
        now_ms,
        now_ms,
    );
}

#[allow(clippy::too_many_arguments)]
fn persist_finals_to_db_at(
    db: &Db,
    session_id: Option<Uuid>,
    channel: Channel,
    origin: &str,
    transcripts: &[Transcript],
    segment_start_ms: &mut u64,
    now_ms: u64,
    speech_started_ms: u64,
) {
    let Some(session_id) = session_id else { return };
    let channel_str = db_channel_str(channel);
    for t in transcripts {
        if !t.is_final {
            continue;
        }
        if t.text.trim().is_empty() {
            continue;
        }
        // Clock the bubble from when this turn landed, not the channel's
        // earlier cursor — that cursor put last-spoken MY SPEECH in the middle.
        let end_ms = now_ms;
        let start_ms = now_ms;
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        let joined = transcript_storage::last_for_session(&conn, session_id)
            .ok()
            .flatten()
            .and_then(|last| {
                // Pause = when this speech *started*, not when the final
                // arrived (the final includes the whole next sentence + EOU).
                let gap = speech_started_ms.saturating_sub(last.created_at_ms);
                if should_continue_line(&last.channel, channel_str, gap, true, &last.text) {
                    let text = merge_notebook_text(&last.text, &t.text);
                    if let Err(e) = transcript_storage::update_segment_text(
                        &conn, last.id, &text, end_ms, now_ms,
                    ) {
                        eprintln!("failed to extend {channel:?} transcript segment: {e}");
                        return None;
                    }
                    return Some(());
                }
                None
            });
        if joined.is_none() {
            if let Err(e) = transcript_storage::insert_segment(
                &conn,
                session_id,
                channel_str,
                origin,
                &t.text,
                start_ms,
                end_ms,
                None,
                true,
                now_ms,
            ) {
                eprintln!("failed to persist {channel:?} transcript segment: {e}");
            }
        }
        drop(conn);
        *segment_start_ms = end_ms;
    }
}

fn report_error(
    app: &AppHandle,
    channel: Channel,
    message: String,
    last_reported: &mut Option<String>,
) {
    eprintln!("{channel:?} transcription error: {message}");
    if last_reported.as_deref() == Some(message.as_str()) {
        return;
    }
    let _ = app.emit(error_event_name(channel), message.clone());
    *last_reported = Some(message);
}

/// `session_id` is Phase 7's incremental-indexing plumbing (`status.md`
/// 7.7): the same `Option<Uuid>` `persist_finals` already receives, threaded
/// one call further so this is the one place both the RAG chunk indexer
/// (`rag::indexer::index_final_transcript`) and fact extraction
/// (`facts::extract` + `storage::facts::upsert_fact`) hook in. `None` (no
/// active session) means both are no-ops, same "nothing to persist without
/// a session" rule `persist_finals` already follows — live events still
/// work.
fn emit_all(
    app: &AppHandle,
    channel: Channel,
    session_id: Option<Uuid>,
    transcripts: Vec<Transcript>,
) {
    for t in transcripts {
        // 2026-08-25: every final on *either* channel is recorded in the
        // in-memory conversation log. Partials prepare (prefetch); finals send.
        if t.is_final {
            crate::transcript_log::append(app, channel, &t.text);
        }
        crate::window_memory::on_transcript(app, channel, &t.text, t.is_final);
        // Mic finals are stored (briefing buffer, exchanges) but never
        // trigger an automatic send — see `conversation::decide`.
        crate::conversation::on_transcript(app, channel, session_id, &t.text, t.is_final);
        if let Some(session_id) = session_id {
            if t.is_final {
                crate::rag::indexer::index_final_transcript(app, session_id, channel, &t.text);
                for fact in crate::facts::facts_from_utterance(&t.text) {
                    let db = app.state::<Db>();
                    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
                    let (source, priority) = match channel {
                        Channel::Mic => (
                            crate::storage::facts::SOURCE_SPOKEN,
                            crate::storage::facts::PRIORITY_SPOKEN,
                        ),
                        Channel::System => (
                            crate::storage::facts::SOURCE_REGEX,
                            crate::storage::facts::PRIORITY_REGEX,
                        ),
                    };
                    if let Err(e) = crate::storage::facts::upsert_fact_ranked(
                        &conn,
                        session_id,
                        &fact.key,
                        &fact.value,
                        source,
                        priority,
                        None,
                        Some(fact.confidence),
                        now_ms(),
                    ) {
                        eprintln!("failed to store extracted fact {:?}: {e}", fact.key);
                    }
                }
            }
        }
        let _ = app.emit(
            event_name(channel),
            TranscriptEvent {
                text: t.text,
                is_final: t.is_final,
            },
        );
    }
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

    /// **Regression test for the "audio captured before the engine is
    /// ready has nowhere to go" gap** (`prepare_route`'s doc comment has
    /// the full story). Installs a route the same way
    /// `commands/stt.rs::start_channel_transcription` now does — before
    /// any engine exists — sends a message through it exactly like a real
    /// `audio/channel.rs::forward_to_stt` call would, and confirms it's
    /// sitting in the queue waiting, not discarded, with no `run()` task
    /// or engine involved at all.
    #[test]
    fn prepare_route_lets_audio_sent_before_the_engine_is_ready_queue_instead_of_being_dropped() {
        let routing = TranscriptionRouting::default();
        let mut prepared = prepare_route(&routing, Channel::Mic);

        let tx =
            routing.mic.lock().unwrap().clone().expect(
                "prepare_route must install a sender immediately, before any engine exists",
            );
        tx.send(StreamMsg::Audio {
            samples: vec![0.1, 0.2, 0.3],
            sample_rate: 16_000,
        })
        .expect("send must succeed — the route is already live");

        // The bridge thread relays std_rx -> tokio_tx asynchronously; give
        // it a moment rather than asserting on the very first poll.
        let mut received = None;
        for _ in 0..50 {
            if let Ok(msg) = prepared.tokio_rx.try_recv() {
                received = Some(msg);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        match received.expect("audio sent before the engine is ready must be queued, not lost") {
            StreamMsg::Audio {
                samples,
                sample_rate,
            } => {
                assert_eq!(samples, vec![0.1, 0.2, 0.3]);
                assert_eq!(sample_rate, 16_000);
            }
            other => panic!("expected an Audio message, got {other:?}"),
        }
    }

    #[test]
    fn persist_finals_to_db_writes_only_final_transcripts() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut segment_start_ms = 0u64;

        let transcripts = vec![
            Transcript {
                text: "still going".into(),
                is_final: false,
            },
            Transcript {
                text: "done talking".into(),
                is_final: true,
            },
            Transcript {
                text: "another partial".into(),
                is_final: false,
            },
        ];
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "local_parakeet",
            &transcripts,
            &mut segment_start_ms,
            1_000,
        );

        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(
            stored.len(),
            1,
            "only the final transcript should be persisted"
        );
        assert_eq!(stored[0].text, "done talking");
        assert!(stored[0].is_final);
    }

    #[test]
    fn persist_finals_to_db_tags_the_correct_channel_and_origin() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut mic_start = 0u64;
        let mut system_start = 0u64;

        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "local_parakeet",
            &[Transcript {
                text: "you: hello".into(),
                is_final: true,
            }],
            &mut mic_start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::System,
            "deepgram",
            &[Transcript {
                text: "them: hi".into(),
                is_final: true,
            }],
            &mut system_start,
            1_100,
        );

        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 2);
        let mic_row = stored.iter().find(|s| s.text == "you: hello").unwrap();
        let system_row = stored.iter().find(|s| s.text == "them: hi").unwrap();
        assert_eq!(mic_row.channel, "mic");
        assert_eq!(mic_row.origin, "local_parakeet");
        assert_eq!(system_row.channel, "system");
        assert_eq!(system_row.origin, "deepgram");
    }

    #[test]
    fn persist_finals_to_db_is_a_no_op_without_a_session_id() {
        let db = Db::open_in_memory();
        let mut segment_start_ms = 0u64;
        // No session row exists at all — if this tried to insert, it would
        // fail the transcript_segments foreign key, proving the no-op path
        // really does return before touching the database.
        persist_finals_to_db(
            &db,
            None,
            Channel::Mic,
            "local_parakeet",
            &[Transcript {
                text: "orphaned".into(),
                is_final: true,
            }],
            &mut segment_start_ms,
            1_000,
        );

        let conn = db.0.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcript_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn persist_finals_to_db_orders_segments_by_advancing_start_time() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut segment_start_ms = 1_000u64;

        // Two finals in one drain batch, as a fast speaker's back-to-back
        // utterances would arrive.
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "local_parakeet",
            &[Transcript {
                text: "first".into(),
                is_final: true,
            }],
            &mut segment_start_ms,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "local_parakeet",
            &[Transcript {
                text: "second".into(),
                is_final: true,
            }],
            &mut segment_start_ms,
            7_000,
        );

        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].text, "first");
        assert_eq!(stored[1].text, "second");
        assert!(
            stored[0].end_ms <= stored[1].start_ms,
            "second segment's window must not start before the first one's ended"
        );
        assert!(
            segment_start_ms >= 1_000,
            "the running cursor must never move backward"
        );
    }

    #[test]
    fn db_channel_str_matches_the_schemas_check_constraint_spelling() {
        assert_eq!(db_channel_str(Channel::Mic), "mic");
        assert_eq!(db_channel_str(Channel::System), "system");
    }

    #[test]
    fn event_names_are_exactly_the_strings_the_frontend_would_listen_for() {
        assert_eq!(event_name(Channel::Mic), "mic-transcript");
        assert_eq!(event_name(Channel::System), "system-transcript");
    }

    #[test]
    fn drain_tick_is_the_agreed_100ms() {
        // 100ms is a backup for cloud results that arrive without new audio;
        // the primary drain is on every audio message. The old 1.5s TDT
        // re-inference cadence made live previews feel late; 200ms was the
        // previous backup and is now halved per the latency plan.
        assert_eq!(DRAIN_TICK_INTERVAL, Duration::from_millis(100));
    }

    #[test]
    fn start_and_stop_toggle_is_running_correctly() {
        let routing = TranscriptionRouting::default();
        assert!(!is_running(&routing, Channel::Mic));
        assert!(!is_running(&routing, Channel::System));

        // Populate the running-state bookkeeping the same way `start()`
        // does, without spinning up a real tokio task or app handle — this
        // test is about `RunningState` bookkeeping, not the async loop.
        {
            let mut running = routing.running.lock().unwrap();
            running.mic.active = Some(Arc::new(AtomicBool::new(false)));
        }
        assert!(is_running(&routing, Channel::Mic));
        assert!(
            !is_running(&routing, Channel::System),
            "starting mic must not affect system"
        );

        stop(&routing, Channel::Mic);
        assert!(!is_running(&routing, Channel::Mic));
    }

    #[tokio::test]
    async fn stop_and_wait_returns_only_after_the_old_task_has_closed() {
        let routing = TranscriptionRouting::default();
        let prepared = prepare_route(&routing, Channel::Mic);
        let flag = prepared.stop_flag.clone();
        let done = prepared.done;
        // Stand-in for `run`: closes 150ms after it is told to stop.
        let finished = Arc::new(AtomicBool::new(false));
        let finished_by_task = finished.clone();
        tokio::spawn(async move {
            while !flag.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            finished_by_task.store(true, Ordering::SeqCst);
            let _ = done.send(true);
        });
        stop_and_wait(&routing, Channel::Mic).await;
        assert!(finished.load(Ordering::SeqCst), "the new session would have overlapped the old one");
        assert!(!is_running(&routing, Channel::Mic));
    }

    #[tokio::test]
    async fn a_plain_stop_is_still_awaited_by_the_next_start() {
        // audio.rs stops synchronously; the next start must still wait.
        let routing = TranscriptionRouting::default();
        let prepared = prepare_route(&routing, Channel::System);
        let done = prepared.done;
        stop(&routing, Channel::System);
        let closer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = done.send(true);
        });
        let began = std::time::Instant::now();
        stop_and_wait(&routing, Channel::System).await;
        assert!(began.elapsed() >= Duration::from_millis(90));
        closer.await.unwrap();
    }

    #[test]
    fn stopping_a_channel_that_was_never_started_is_a_harmless_no_op() {
        let routing = TranscriptionRouting::default();
        stop(&routing, Channel::Mic);
        stop(&routing, Channel::System);
        assert!(!is_running(&routing, Channel::Mic));
    }

    #[test]
    fn the_two_channels_route_independently() {
        let routing = TranscriptionRouting::default();
        let (tx, _rx) = sync_mpsc::channel();
        *routing.mic.lock().unwrap() = Some(tx);
        assert!(routing.mic.lock().unwrap().is_some());
        assert!(
            routing.system.lock().unwrap().is_none(),
            "setting the mic route must not populate the system route"
        );
    }

    #[test]
    fn a_two_second_pause_then_a_long_sentence_stays_one_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "I was thinking".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db_at(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "about Kubernetes and how we ship Helm charts".into(),
                is_final: true,
            }],
            &mut start,
            9_000,
            3_000,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(
            stored.len(),
            1,
            "the 2s think-pause is the gap, not the 8s of talking after"
        );
        assert_eq!(
            stored[0].text,
            "I was thinking about Kubernetes and how we ship Helm charts"
        );
    }

    #[test]
    fn four_second_same_channel_pause_extends_the_notebook_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "I was thinking".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "about Kubernetes".into(),
                is_final: true,
            }],
            &mut start,
            5_000,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].text, "I was thinking about Kubernetes");
    }

    #[test]
    fn one_second_same_channel_pause_extends_the_notebook_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "I was thinking".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "about Kubernetes".into(),
                is_final: true,
            }],
            &mut start,
            2_000,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].text, "I was thinking about Kubernetes");
    }

    #[test]
    fn a_gap_over_five_seconds_is_a_new_notebook_row() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "first thought".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "later thought".into(),
                is_final: true,
            }],
            &mut start,
            6_500,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].text, "first thought");
        assert_eq!(stored[1].text, "later thought");
    }

    #[test]
    fn other_party_in_between_blocks_joining_my_speech() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut mic = 0u64;
        let mut sys = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "hello".into(),
                is_final: true,
            }],
            &mut mic,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::System,
            "speechmatics",
            &[Transcript {
                text: "can you explain".into(),
                is_final: true,
            }],
            &mut sys,
            1_200,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "Kubernetes".into(),
                is_final: true,
            }],
            &mut mic,
            1_400,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 3);
        assert_eq!(stored[0].text, "hello");
        assert_eq!(stored[1].text, "can you explain");
        assert_eq!(stored[2].text, "Kubernetes");
    }

    #[test]
    fn a_repeated_suffix_does_not_duplicate_on_join() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "I was thinking about Kubernetes".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "about Kubernetes".into(),
                is_final: true,
            }],
            &mut start,
            2_000,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].text, "I was thinking about Kubernetes");
    }

    #[test]
    fn ack_only_line_is_not_extended() {
        let db = Db::open_in_memory();
        let session_id = seeded_session(&db);
        let mut start = 0u64;
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "Yes.".into(),
                is_final: true,
            }],
            &mut start,
            1_000,
        );
        persist_finals_to_db(
            &db,
            Some(session_id),
            Channel::Mic,
            "speechmatics",
            &[Transcript {
                text: "I started in 2019.".into(),
                is_final: true,
            }],
            &mut start,
            1_400,
        );
        let conn = db.0.lock().unwrap();
        let stored = transcript_storage::list_for_session(&conn, session_id).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].text, "Yes.");
        assert_eq!(stored[1].text, "I started in 2019.");
    }
}
