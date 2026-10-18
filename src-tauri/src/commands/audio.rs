use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::aec::AecHub;
use crate::audio::channel::Channel;
use crate::audio::device_util;
use crate::audio::device_watch::{DefaultDeviceKind, DeviceChangeWatcher};
use crate::audio::mic::{self, InputDevice, MicCaptureHandle};
use crate::audio::system::{self, CaptureInfo, OutputDevice, SystemCaptureHandle};
use crate::pipeline::{self, TranscriptionRouting};
use crate::storage::{settings, Db};

/// Which device id each channel currently has pinned, persisted so a choice
/// survives a restart — mirrors `commands/stt.rs`'s `ENGINE_SETTING_KEY`
/// shape exactly. Absent (the ordinary starting state) means "follow the OS
/// default", matching `start_mic_capture`/`start_system_audio_capture`'s own
/// `device: None` meaning before this feature existed.
const MIC_DEVICE_SETTING_KEY: &str = "mic_device_id";
const SYSTEM_DEVICE_SETTING_KEY: &str = "system_device_id";

fn warn_if_open_loudspeaker(app: &AppHandle, output_id: Option<&str>, output_name: Option<&str>) {
    let aec = app.state::<AecHub>();
    if aec.set_enabled_for_output(output_id, output_name) {
        let notice = device_util::SPEAKER_ECHO_NOTICE.to_string();
        let _ = app.emit(Channel::Mic.notice_event(), notice.clone());
        let _ = app.emit(Channel::System.notice_event(), notice);
    }
}

/// Plays locally before a call, through the output the check is listening to.
#[tauri::command]
pub async fn play_system_audio_test(app: AppHandle, device: Option<String>) -> Result<(), String> {
    if crate::active_session::get(&app).is_some() {
        return Err("A call is running. Test audio before starting your next call.".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || crate::audio::test_sound::play(device.as_deref(), worker_cancelled));
    match tokio::time::timeout(std::time::Duration::from_secs(3), worker).await {
        Ok(result) => result.map_err(|_| "Couldn't finish the test sound. Try again.".to_string())?,
        Err(_) => {
            cancelled.store(true, Ordering::SeqCst);
            Err("Your output device did not respond to the test sound. Check the device and try again.".into())
        }
    }
}

/// Holds the running mic capture, if any. `None` means not capturing.
#[derive(Default)]
pub struct AudioState(pub Mutex<Option<MicCaptureHandle>>);

// CoreAudio can wait for permissions/device replies. Keep those waits off the UI thread.
#[tauri::command(async)]
pub fn start_mic_capture(
    app: AppHandle,
    state: State<AudioState>,
    routing: State<TranscriptionRouting>,
    device: Option<String>,
) -> Result<CaptureInfo, String> {
    // Holds the channel's op lock for this whole call — see
    // `device_follow`'s module comment on why a manual start must never run
    // concurrently with a device-follow restart's own stop-then-start cycle
    // on the same channel.
    device_follow::with_op_lock(&app, Channel::Mic, || {
        start_mic_capture_body(&app, &state, &routing, device)
    })
}

/// The actual work of [`start_mic_capture`], factored out so
/// `device_follow::perform_restart` can call it directly while it already
/// holds the channel's op lock — it must never call the `#[tauri::command]`
/// wrapper above, which takes that same (non-reentrant) lock itself and
/// would deadlock.
fn start_mic_capture_body(
    app: &AppHandle,
    state: &AudioState,
    routing: &TranscriptionRouting,
    device: Option<String>,
) -> Result<CaptureInfo, String> {
    let info = {
        let mut guard = state
            .0
            .lock()
            .map_err(|_| "audio state poisoned".to_string())?;
        if let Some(existing) = guard.as_ref() {
            // Idempotent, matching start_system_audio_capture. Returning the
            // running stream's info rather than `()` is what lets the Audio
            // Inspector show sample rate / channel count / live device for
            // the microphone the same way it already could for system audio.
            return Ok(existing.info.clone());
        }
        let handle = mic::start(
            app.clone(),
            device.clone(),
            routing.mic.clone(),
            (*app.state::<AecHub>()).clone(),
        )?;
        let info = handle.info.clone();
        *guard = Some(handle);
        info
    };
    // Auto-follow only ever applies when the caller didn't pin a specific
    // device (the Part A picker case) — see `device_follow`'s module
    // comment. A pinned device disarms any previously-armed watcher for
    // this channel, matching "if they picked a device specifically, Aside
    // should keep using exactly that regardless of what the OS default
    // becomes."
    if device.is_none() {
        device_follow::note_current_device(
            app,
            Channel::Mic,
            device_util::default_input_device_id(),
        );
        device_follow::arm(app, Channel::Mic);
    } else {
        device_follow::disarm(app, Channel::Mic);
    }
    warn_if_open_loudspeaker(
        app,
        device_util::default_output_device_id().as_deref(),
        device_util::default_output_device_name().as_deref(),
    );
    Ok(info)
}

#[tauri::command(async)]
pub fn stop_mic_capture(
    app: AppHandle,
    state: State<AudioState>,
    routing: State<TranscriptionRouting>,
) -> Result<(), String> {
    device_follow::with_op_lock(&app, Channel::Mic, || {
        device_follow::stop_mic_capture_only(&state, &routing, true)?;
        // Disarm on every manual stop, armed or not (a no-op if it wasn't) —
        // leaving a live CoreAudio listener registered with nothing left to
        // restart would be a resource leak for the rest of the session.
        device_follow::disarm(&app, Channel::Mic);
        Ok(())
    })
}

/// Holds the running system-audio capture, if any. Separate from
/// [`AudioState`] on purpose: the two channels start, stop, and fail
/// independently, and the mic must keep working when system audio can't (an
/// unsupported macOS version, a denied permission, an untappable device).
#[derive(Default)]
pub struct SystemAudioState(pub Mutex<Option<SystemCaptureHandle>>);

#[tauri::command(async)]
pub fn start_system_audio_capture(
    app: AppHandle,
    state: State<SystemAudioState>,
    routing: State<TranscriptionRouting>,
    device: Option<String>,
) -> Result<CaptureInfo, String> {
    device_follow::with_op_lock(&app, Channel::System, || {
        start_system_audio_capture_body(&app, &state, &routing, device)
    })
}

/// The actual work of [`start_system_audio_capture`] — see
/// `start_mic_capture_body`'s doc comment for why this exists and why
/// `perform_restart` must call this, never the `#[tauri::command]` wrapper.
fn start_system_audio_capture_body(
    app: &AppHandle,
    state: &SystemAudioState,
    routing: &TranscriptionRouting,
    device: Option<String>,
) -> Result<CaptureInfo, String> {
    let info = {
        let mut guard = state
            .0
            .lock()
            .map_err(|_| "system audio state poisoned".to_string())?;
        if let Some(existing) = guard.as_ref() {
            // Idempotent, matching start_mic_capture. Returning the running
            // stream's info rather than an error means a double-click on the UI
            // toggle is harmless instead of surfacing a scary message.
            return Ok(existing.info.clone());
        }
        let handle = system::start(
            app.clone(),
            device.clone(),
            routing.system.clone(),
            (*app.state::<AecHub>()).clone(),
        )?;
        let info = handle.info.clone();
        *guard = Some(handle);
        info
    };
    if device.is_none() {
        device_follow::note_current_device(app, Channel::System, info.device_id.clone());
        device_follow::arm(app, Channel::System);
    } else {
        device_follow::disarm(app, Channel::System);
    }
    warn_if_open_loudspeaker(app, info.device_id.as_deref(), Some(&info.device_name));
    Ok(info)
}

/// Whether each channel is currently capturing — the read half of the
/// capture-status surface (`commands::capture::get_capture_status`). Cheap
/// lock-and-peek, no side effects. A poisoned lock reports `false` rather
/// than erroring: the status strip must always be able to render something.
pub(crate) fn is_mic_capturing(state: &AudioState) -> bool {
    state.0.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub(crate) fn is_system_capturing(state: &SystemAudioState) -> bool {
    state.0.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Live stream info for a channel that is already capturing, or `None`
/// when that ear is idle. The Audio Inspector uses this on mount so a
/// remount does not have to call start (which is idempotent, but a query
/// is the honest read).
#[tauri::command(async)]
pub fn get_mic_capture_info(state: State<AudioState>) -> Result<Option<CaptureInfo>, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "audio state poisoned".to_string())?;
    Ok(guard.as_ref().map(|h| h.info.clone()))
}

#[tauri::command(async)]
pub fn get_system_capture_info(
    state: State<SystemAudioState>,
) -> Result<Option<CaptureInfo>, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "system audio state poisoned".to_string())?;
    Ok(guard.as_ref().map(|h| h.info.clone()))
}

#[tauri::command(async)]
pub fn stop_system_audio_capture(
    app: AppHandle,
    state: State<SystemAudioState>,
    routing: State<TranscriptionRouting>,
) -> Result<(), String> {
    device_follow::with_op_lock(&app, Channel::System, || {
        device_follow::stop_system_audio_capture_only(&state, &routing, true)?;
        device_follow::disarm(&app, Channel::System);
        Ok(())
    })
}

/// Lists the output devices that could be captured, so the UI can offer a
/// choice when the default is not the one carrying the call.
#[tauri::command(async)]
pub fn list_output_devices() -> Result<Vec<OutputDevice>, String> {
    system::list_output_devices()
}

/// Lists the input devices that could serve as the microphone, so the UI can
/// offer a choice instead of always following the OS default.
#[tauri::command(async)]
pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
    mic::list_input_devices()
}

// --- Device-choice persistence ---
//
// Same shape as `commands/stt.rs`'s `get_stt_engine`/`set_stt_engine`: a
// plain settings-table round trip, no dedicated storage module needed since
// `storage::settings` is already generic key/value. `device: None` on the
// setter clears the pin (goes back to following the OS default) rather than
// pinning a snapshot of whatever the default happened to be at that moment —
// `resolve_device`'s own fallback already tracks the live OS default.

/// The mic device the user has pinned, or `None` to follow the OS default.
#[tauri::command]
pub fn get_mic_device(db: State<Db>) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::get(&conn, MIC_DEVICE_SETTING_KEY)
}

#[tauri::command]
pub fn set_mic_device(db: State<Db>, device: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    match device {
        Some(id) => settings::set(&conn, MIC_DEVICE_SETTING_KEY, &id),
        None => settings::delete(&conn, MIC_DEVICE_SETTING_KEY),
    }
}

/// The system-audio (output) device the user has pinned, or `None` to follow
/// the OS default.
#[tauri::command]
pub fn get_system_device(db: State<Db>) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::get(&conn, SYSTEM_DEVICE_SETTING_KEY)
}

#[tauri::command]
pub fn set_system_device(db: State<Db>, device: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    match device {
        Some(id) => settings::set(&conn, SYSTEM_DEVICE_SETTING_KEY, &id),
        None => settings::delete(&conn, SYSTEM_DEVICE_SETTING_KEY),
    }
}

/// Live OS-default-device auto-follow: when a channel isn't pinned to a
/// specific device, notice a CoreAudio default-device change and switch to
/// it automatically, the way Zoom/Meet/Teams already do.
///
/// Deliberately its own inner module rather than spread across this file's
/// top level: it is the one part of `commands/audio.rs` with real
/// concurrency (a CoreAudio notification thread, a spawned restart thread,
/// and this module's own command handlers, all touching shared state), and
/// keeping its atomics/locking together in one place made the guard logic
/// below easier to get right and easier to review than interleaving it with
/// the plain start/stop commands above.
///
/// ## Design (for review)
///
/// Each channel gets one [`FollowChannelState`] bucket
/// (`DeviceFollowState::mic` / `::system`), holding:
/// - `armed: Mutex<Option<ArmedFollow>>` — `Some` while a
///   [`DeviceChangeWatcher`] is registered for this channel. Its mere
///   presence/absence *is* the "is this channel currently following"
///   signal — see `is_armed` — rather than a second flag that could drift
///   from it.
/// - `last_known_device` / `restarting` / `dirty` / `consecutive_failures` —
///   shared `Arc`s that outlive any single arm/disarm cycle, cloned into
///   the CoreAudio callback closure so they stay valid after `arm` returns.
///
/// **Same-physical-device suppression.** Every restart attempt starts by
/// comparing the OS's live default id against `last_known_device` with
/// [`device_util::same_physical_device`] (moved there from `system.rs` for
/// exactly this reuse). A spurious re-announce of the device already in use
/// — observed on this project's own hardware as a role-suffix change on a
/// Bluetooth reconnect — is suppressed rather than tearing down and
/// rebuilding a perfectly good stream.
///
/// **Debounce / reentrancy.** The CoreAudio callback (`on_default_changed`)
/// does only an atomic swap on `restarting` and returns immediately — never
/// blocking CoreAudio's notification thread. If a restart is already in
/// flight, the fresh notification just sets `dirty` and returns. The
/// in-flight `restart_loop` checks `dirty` after each attempt and, if set,
/// clears it and re-evaluates against whatever the OS default is *now*
/// before deciding whether to stop — this is what keeps a Bluetooth device's
/// several rapid-fire notifications during reconnect from being dropped on
/// the floor if the last one arrives while an earlier restart is still
/// running.
///
/// **Failure cap.** `consecutive_failures` counts restart attempts that
/// actually tried and failed (not suppressed ones, not ones cancelled by a
/// manual stop). At [`MAX_CONSECUTIVE_FAILURES`] in a row — a flapping
/// virtual/aggregate device is the expected real-world trigger — the channel
/// disarms itself and emits `channel.error_event()` so the frontend's
/// existing notice/error plumbing surfaces it, rather than retrying forever
/// against a device that clearly isn't going to work.
///
/// **Race with a manual Stop click and a manual re-pin — both closed by one
/// combined lock.**
///
/// This module originally shipped with a check-act-recheck mitigation here
/// instead of a real lock (`perform_restart` checking `is_armed` immediately
/// before and after the "start capture again" step) — explicitly flagged at
/// the time as *not* a proof of exclusion, since no single lock spanned both
/// `AudioState`/`SystemAudioState` and `DeviceFollowState`. Adversarial
/// review confirmed that gap was real by tracing two concrete
/// interleavings, both reachable because a manual command and a
/// restart-in-progress could genuinely run their "stop the old stream,
/// start a new one" steps concurrently against the same channel:
///
/// 1. **A manual re-pin landing mid-restart.** The user picks a specific
///    device (`SttBench.tsx`'s `chooseDevice`, which itself does
///    stop-then-start) while a background restart is between its own stop
///    and start. The restart's post-start recheck of `is_armed` reads
///    `false` (the user's pinned start legitimately disarmed follow-mode)
///    and — reading that as "a plain Stop landed" — proceeds to stop what
///    it assumes is its own just-started capture, which by then is actually
///    the device the user explicitly just chose.
/// 2. **A plain Stop landing in the same window, with the opposite bug.**
///    If the user's Stop (stop + disarm) runs to completion *entirely*
///    between the restart's pre-start `is_armed` check and its start call's
///    own internal `arm()`, the restart's own start call re-registers a
///    watcher and re-arms the channel — silently undoing the user's Stop.
///    The post-start recheck then reads `true` (armed again, by the
///    restart's own hand) and treats this as a clean success, leaving the
///    mic running against explicit user intent.
///
/// Neither needs exotic timing: the window is "however long
/// `mic::start`/`system::start` takes to open a real device," not a handful
/// of CPU instructions — comfortably reachable by a user clicking the
/// device dropdown at an unlucky moment.
///
/// **The fix:** each channel gets one [`FollowChannelState::op_lock`], held
/// for the *entire* duration of every operation that can mutate that
/// channel's capture state — a manual `start_mic_capture`/`stop_mic_capture`
/// command, and a device-follow restart's whole stop-then-start cycle
/// (`perform_restart`) — never just around one field access. Because these
/// operations now cannot interleave at all, none of the three races above
/// can occur: whichever operation acquires the lock first runs to
/// completion (stop *and* start, if it does both) before any other
/// operation on that channel can begin. `perform_restart` and the
/// `#[tauri::command]` wrappers both take this lock, so `perform_restart`
/// calls the `_body`/`_only` helpers directly rather than the public
/// commands — calling a command that re-takes the same (non-reentrant)
/// `std::sync::Mutex` from inside a closure already holding it would
/// deadlock.
///
/// One race the op lock does **not** cover, stated honestly: `lib.rs`'s
/// shutdown path calls `disarm_device_follow` and then reaches into
/// `AudioState`/`SystemAudioState` directly, bypassing `with_op_lock`
/// entirely (it has to — the process is on its way out and cannot afford to
/// block waiting for an in-flight restart to finish). If shutdown's disarm
/// lands *before* a concurrently-in-flight restart's own start step calls
/// `arm()`, that `arm()` call silently re-arms the channel shutdown just
/// disarmed, and the restart's post-start `is_armed` check then reads
/// `true` (armed again, by its own hand) instead of catching the shutdown.
/// Either way the capture itself still gets torn down — shutdown's direct
/// `AudioState`/`SystemAudioState` access runs right after and stops
/// whatever is there regardless of `armed` — so nothing is left running;
/// what can be left is a freshly re-registered CoreAudio listener with
/// nothing to restart. That is inert in practice: the process exits
/// immediately afterward (`app.run`'s closure returns and the OS reclaims
/// everything), so the stray listener never gets a chance to fire.
/// Documented rather than silently depended upon.
mod device_follow {
    use super::*;

    /// How many consecutive failed restart attempts a channel tolerates
    /// before giving up and disarming itself. Chosen to absorb a genuinely
    /// transient failure (the new default device taking a moment to finish
    /// initializing after a Bluetooth reconnect) without retrying forever
    /// against a device that just isn't going to work.
    const MAX_CONSECUTIVE_FAILURES: u32 = 3;

    /// A currently-registered watcher for one channel. Its mere presence in
    /// `FollowChannelState::armed` is what "this channel is following the OS
    /// default" means — see `is_armed`.
    struct ArmedFollow {
        _watcher: DeviceChangeWatcher,
    }

    /// Per-channel auto-follow bookkeeping. See this module's doc comment.
    #[derive(Default)]
    struct FollowChannelState {
        armed: Mutex<Option<ArmedFollow>>,
        last_known_device: Arc<Mutex<Option<String>>>,
        restarting: Arc<AtomicBool>,
        dirty: Arc<AtomicBool>,
        consecutive_failures: Arc<AtomicU32>,
        /// Serializes every full start-or-stop *operation* on this channel —
        /// a manual `start_mic_capture`/`stop_mic_capture` command, and a
        /// device-follow restart's whole stop-then-start cycle — against
        /// every other one, at the granularity of "one complete operation"
        /// rather than "one field access." See this module's doc comment
        /// for the three races this closes. Access it only through
        /// `with_op_lock`/`lock_op`, never directly, so every caller goes
        /// through the same "hold it for the whole operation" discipline.
        op_lock: Mutex<()>,
    }

    /// Tauri-managed state, one instance for the app — see `lib.rs`'s
    /// `.manage(...)` call.
    #[derive(Default)]
    pub struct DeviceFollowState {
        mic: FollowChannelState,
        system: FollowChannelState,
    }

    fn bucket(follow: &DeviceFollowState, channel: Channel) -> &FollowChannelState {
        match channel {
            Channel::Mic => &follow.mic,
            Channel::System => &follow.system,
        }
    }

    /// Runs `f` with `follow`'s op lock held for `f`'s entire duration.
    /// Takes the already-resolved per-channel bucket, not an `AppHandle` +
    /// `Channel`, so this — the actual mutual-exclusion primitive the whole
    /// fix depends on — is unit-testable directly with a hand-built
    /// [`FollowChannelState`] (see this module's tests), not only provable
    /// through a running Tauri app. [`with_op_lock`] is the convenience
    /// wrapper real call sites use.
    ///
    /// A plain, non-reentrant `std::sync::Mutex`: nesting two calls for the
    /// same channel on the same thread deadlocks. That is exactly why
    /// `perform_restart` calls `start_mic_capture_body`/
    /// `start_system_audio_capture_body`/`stop_..._only` directly instead of
    /// the public `#[tauri::command]` wrappers, which take this same lock.
    fn lock_op<T>(follow: &FollowChannelState, f: impl FnOnce() -> T) -> T {
        let _guard = follow.op_lock.lock().unwrap_or_else(|p| p.into_inner());
        f()
    }

    /// [`lock_op`] resolved through the app's managed [`DeviceFollowState`] —
    /// what every real call site (the `#[tauri::command]` wrappers in the
    /// parent module, and `perform_restart` below) actually calls.
    pub(super) fn with_op_lock<T>(app: &AppHandle, channel: Channel, f: impl FnOnce() -> T) -> T {
        let follow = app.state::<DeviceFollowState>();
        lock_op(bucket(&follow, channel), f)
    }

    /// True while `channel` currently has a live watcher registered. This
    /// alone is the "should a background restart keep going / should a
    /// notification be acted on" signal — see this module's doc comment.
    fn is_armed(app: &AppHandle, channel: Channel) -> bool {
        let follow = app.state::<DeviceFollowState>();
        let is_some = bucket(&follow, channel)
            .armed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some();
        is_some
    }

    /// Records the device id a channel is (believed to be) actually
    /// capturing from right now, for the same-physical-device suppression
    /// check on the next notification. Called once right after a successful
    /// (re)start.
    pub fn note_current_device(app: &AppHandle, channel: Channel, device_id: Option<String>) {
        let follow = app.state::<DeviceFollowState>();
        *bucket(&follow, channel)
            .last_known_device
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = device_id;
    }

    /// Arms auto-follow for `channel`: registers a [`DeviceChangeWatcher`]
    /// and resets its failure count for a clean slate. A registration
    /// failure (Windows, or a CoreAudio error) is logged and treated as
    /// "auto-follow just doesn't activate this time" — never fatal to the
    /// capture that's already running, matching this feature's "capture
    /// works the same as before if this can't be built" requirement.
    pub fn arm(app: &AppHandle, channel: Channel) {
        let follow = app.state::<DeviceFollowState>();
        let b = bucket(&follow, channel);
        b.consecutive_failures.store(0, Ordering::SeqCst);

        let restarting = b.restarting.clone();
        let dirty = b.dirty.clone();
        let consecutive_failures = b.consecutive_failures.clone();
        let last_known_device = b.last_known_device.clone();
        let cb_app = app.clone();

        let kind = match channel {
            Channel::Mic => DefaultDeviceKind::Input,
            Channel::System => DefaultDeviceKind::Output,
        };

        let watcher = DeviceChangeWatcher::start(kind, move || {
            on_default_changed(
                cb_app.clone(),
                channel,
                restarting.clone(),
                dirty.clone(),
                consecutive_failures.clone(),
                last_known_device.clone(),
            );
        });

        match watcher {
            Ok(w) => {
                *b.armed.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(ArmedFollow { _watcher: w });
            }
            Err(e) => {
                eprintln!("device_watch: not auto-following the {channel:?} default device: {e}");
            }
        }
    }

    /// Disarms auto-follow for `channel`, if armed. A no-op otherwise —
    /// every caller (a manual stop, a pin, the failure cap, app shutdown)
    /// treats "already disarmed" as fine, matching this project's existing
    /// idempotent-stop convention (`stop_mic_capture` etc.).
    pub fn disarm(app: &AppHandle, channel: Channel) {
        let follow = app.state::<DeviceFollowState>();
        let mut guard = bucket(&follow, channel)
            .armed
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if guard.take().is_some() {
            eprintln!("device_watch: disarmed auto-follow for {channel:?}");
        }
    }

    /// Disarms both channels — called once at app shutdown. See `lib.rs`.
    pub fn disarm_all(app: &AppHandle) {
        disarm(app, Channel::Mic);
        disarm(app, Channel::System);
    }

    // ---- the raw stop, without the follow-state side effect ---------------
    //
    // The public `stop_mic_capture`/`stop_system_audio_capture` commands
    // disarm as part of an ordinary user-initiated stop — see this module's
    // doc comment. `perform_restart`'s own internal "stop the old stream"
    // step must NOT do that (it would immediately defeat its own following
    // restart), so it calls these instead. Kept `pub(super)` rather than
    // fully private only because the two `#[tauri::command]` wrappers above
    // this module live in the parent module and need them too.

    pub(super) fn stop_mic_capture_only(
        state: &State<AudioState>,
        routing: &State<TranscriptionRouting>,
        stop_stt: bool,
    ) -> Result<(), String> {
        let mut guard = state
            .0
            .lock()
            .map_err(|_| "audio state poisoned".to_string())?;
        if let Some(handle) = guard.take() {
            handle.stop();
        }
        // Device-follow restarts capture on the same ear; killing STT here
        // is how the meter keeps moving with no new words.
        if stop_stt {
            pipeline::stop(routing, Channel::Mic);
        }
        Ok(())
    }

    pub(super) fn stop_system_audio_capture_only(
        state: &State<SystemAudioState>,
        routing: &State<TranscriptionRouting>,
        stop_stt: bool,
    ) -> Result<(), String> {
        let mut guard = state
            .0
            .lock()
            .map_err(|_| "system audio state poisoned".to_string())?;
        if let Some(handle) = guard.take() {
            handle.stop();
        }
        if stop_stt {
            pipeline::stop(routing, Channel::System);
        }
        Ok(())
    }

    // ---- reacting to a fired notification ----------------------------------

    /// Runs on a CoreAudio notification thread — must return fast and never
    /// block (see `device_watch`'s doc comment). Does only an atomic
    /// claim-or-coalesce and, when it wins the claim, spawns a plain OS
    /// thread to do the real work.
    fn on_default_changed(
        app: AppHandle,
        channel: Channel,
        restarting: Arc<AtomicBool>,
        dirty: Arc<AtomicBool>,
        consecutive_failures: Arc<AtomicU32>,
        last_known_device: Arc<Mutex<Option<String>>>,
    ) {
        if restarting.swap(true, Ordering::SeqCst) {
            // A restart from an earlier fire is already in flight. Mark
            // dirty so it loops once more against the settled state once it
            // finishes, instead of this notification being silently
            // dropped — see the module doc comment on Bluetooth reconnects
            // firing the property multiple times in quick succession.
            dirty.store(true, Ordering::SeqCst);
            eprintln!(
                "device_watch: {channel:?} restart already in flight, coalescing this notification"
            );
            return;
        }
        eprintln!("device_watch: {channel:?} OS default device changed, evaluating restart");
        std::thread::spawn(move || {
            restart_loop(
                app,
                channel,
                restarting,
                dirty,
                consecutive_failures,
                last_known_device,
            );
        });
    }

    /// Pure decision: is the OS's current default genuinely a different
    /// piece of hardware than the one this channel is already using? `true`
    /// means "go ahead and restart."
    fn should_restart(currently_using: Option<&str>, new_default: Option<&str>) -> bool {
        !device_util::same_physical_device(currently_using, new_default)
    }

    /// Pure decision: has this channel failed enough consecutive times in a
    /// row to stop trying?
    fn should_give_up(consecutive_failures: u32) -> bool {
        consecutive_failures >= MAX_CONSECUTIVE_FAILURES
    }

    enum RestartOutcome {
        /// A manual stop landed mid-restart; not counted as a failure.
        CancelledByManualStop,
        Failed(String),
    }

    /// Runs on its own dedicated thread (never the CoreAudio notification
    /// thread, never a tokio worker) for as long as it takes to settle —
    /// see the module doc comment for the coalescing loop this implements.
    fn restart_loop(
        app: AppHandle,
        channel: Channel,
        restarting: Arc<AtomicBool>,
        dirty: Arc<AtomicBool>,
        consecutive_failures: Arc<AtomicU32>,
        last_known_device: Arc<Mutex<Option<String>>>,
    ) {
        loop {
            dirty.store(false, Ordering::SeqCst);

            if !is_armed(&app, channel) {
                eprintln!("device_watch: {channel:?} no longer armed (stopped or pinned meanwhile), dropping this restart");
                break;
            }

            let new_id = match channel {
                Channel::Mic => device_util::default_input_device_id(),
                Channel::System => device_util::default_output_device_id(),
            };
            let old_id = last_known_device
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();

            if should_restart(old_id.as_deref(), new_id.as_deref()) {
                eprintln!("device_watch: {channel:?} default changed {old_id:?} -> {new_id:?}; restarting capture");
                match perform_restart(&app, channel) {
                    Ok(resolved) => {
                        consecutive_failures.store(0, Ordering::SeqCst);
                        let name = match channel {
                            Channel::Mic => device_util::default_input_device_name(),
                            Channel::System => device_util::default_output_device_name(),
                        };
                        *last_known_device.lock().unwrap_or_else(|p| p.into_inner()) =
                            resolved.or(new_id);
                        let _ = app.emit(
                            channel.notice_event(),
                            format!(
                                "Automatically switched to the new system default device{}.",
                                name.map(|n| format!(" ({n})")).unwrap_or_default()
                            ),
                        );
                    }
                    Err(RestartOutcome::CancelledByManualStop) => {
                        eprintln!("device_watch: {channel:?} auto-restart cancelled — capture was stopped manually mid-restart");
                        break;
                    }
                    Err(RestartOutcome::Failed(e)) => {
                        let failures = consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
                        eprintln!(
                            "device_watch: {channel:?} auto-restart failed ({failures}/{MAX_CONSECUTIVE_FAILURES}): {e}"
                        );
                        let _ = app.emit(
                            channel.notice_event(),
                            format!(
                                "Could not automatically switch to the new default device: {e}"
                            ),
                        );
                        if should_give_up(failures) {
                            let _ = app.emit(
                                channel.error_event(),
                                format!(
                                    "Gave up automatically following the system default device \
                                     after {failures} failed attempts in a row. Pick a device \
                                     manually (or press Listen again) to resume."
                                ),
                            );
                            disarm(&app, channel);
                            break;
                        }
                    }
                }
            } else {
                eprintln!(
                    "device_watch: {channel:?} default is still the same physical device ({new_id:?}); suppressing restart"
                );
                *last_known_device.lock().unwrap_or_else(|p| p.into_inner()) = new_id;
            }

            if !dirty.load(Ordering::SeqCst) {
                break;
            }
            eprintln!(
                "device_watch: {channel:?} default changed again while restarting; re-evaluating"
            );
        }
        restarting.store(false, Ordering::SeqCst);
    }

    /// Stops the currently-running capture and starts it again with
    /// `device: None` — which naturally resolves to whatever the OS default
    /// is *at that moment* (see `mic::resolve_device`/`system::resolve_device`),
    /// so there is no need to thread a specific "new device id" through this
    /// call. Calls the same `_body`/`_only` logic the public start/stop
    /// commands use, rather than re-implementing capture teardown/setup
    /// here — but never the commands themselves, since this whole function
    /// runs inside `with_op_lock`, and the commands take that same lock
    /// (see this module's doc comment on why: `perform_restart`'s
    /// stop-then-start must run as one uninterruptible operation, not two
    /// separately-lockable halves).
    fn perform_restart(
        app: &AppHandle,
        channel: Channel,
    ) -> Result<Option<String>, RestartOutcome> {
        with_op_lock(app, channel, || match channel {
            Channel::Mic => {
                stop_mic_capture_only(
                    &app.state::<AudioState>(),
                    &app.state::<TranscriptionRouting>(),
                    false,
                )
                .map_err(RestartOutcome::Failed)?;
                if !is_armed(app, channel) {
                    return Err(RestartOutcome::CancelledByManualStop);
                }
                super::start_mic_capture_body(
                    app,
                    &app.state::<AudioState>(),
                    &app.state::<TranscriptionRouting>(),
                    None,
                )
                .map_err(RestartOutcome::Failed)?;
                if !is_armed(app, channel) {
                    // Only reachable via the shutdown path now (see this
                    // module's doc comment) — every manual start/stop is
                    // serialized out by the op lock this whole function is
                    // already running inside of. Still stop what was just
                    // started rather than leave it running.
                    let _ = stop_mic_capture_only(
                        &app.state::<AudioState>(),
                        &app.state::<TranscriptionRouting>(),
                        true,
                    );
                    disarm(app, channel);
                    return Err(RestartOutcome::CancelledByManualStop);
                }
                Ok(device_util::default_input_device_id())
            }
            Channel::System => {
                stop_system_audio_capture_only(
                    &app.state::<SystemAudioState>(),
                    &app.state::<TranscriptionRouting>(),
                    false,
                )
                .map_err(RestartOutcome::Failed)?;
                if !is_armed(app, channel) {
                    return Err(RestartOutcome::CancelledByManualStop);
                }
                let info = super::start_system_audio_capture_body(
                    app,
                    &app.state::<SystemAudioState>(),
                    &app.state::<TranscriptionRouting>(),
                    None,
                )
                .map_err(RestartOutcome::Failed)?;
                if !is_armed(app, channel) {
                    let _ = stop_system_audio_capture_only(
                        &app.state::<SystemAudioState>(),
                        &app.state::<TranscriptionRouting>(),
                        true,
                    );
                    disarm(app, channel);
                    return Err(RestartOutcome::CancelledByManualStop);
                }
                Ok(info.device_id)
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // ---- pure decision logic ------------------------------------------
        //
        // `should_restart`/`should_give_up` are the only pieces of this
        // module's guard logic that are plain functions over plain values —
        // everything else (the CoreAudio callback, the spawned restart
        // thread, the Tauri `State` lookups) needs a running app and/or a
        // live OS default-device change to exercise for real, and is not
        // unit-tested here. See this phase's report for exactly what that
        // leaves for a live owner test to confirm.

        #[test]
        fn should_restart_is_false_when_nothing_really_changed() {
            assert!(!should_restart(Some("abc"), Some("abc")));
        }

        #[test]
        fn should_restart_is_false_across_a_role_suffix_change_on_the_same_hardware() {
            // The literal case this guard exists for: a Bluetooth device's
            // default-device notification re-announcing the same physical
            // headset under its other CoreAudio role id.
            assert!(!should_restart(
                Some("38-C4-3A-5A-28-F0:input"),
                Some("38-C4-3A-5A-28-F0:output")
            ));
        }

        #[test]
        fn should_restart_is_true_for_a_genuinely_different_device() {
            assert!(should_restart(
                Some("built-in-mic"),
                Some("usb-interface-42")
            ));
        }

        #[test]
        fn should_restart_is_true_when_the_previous_device_is_unknown() {
            // Never tracked a device yet (first notification right after
            // arming, before `note_current_device` ever ran) — proceeding is
            // the safe default; silently refusing to follow because we
            // "can't prove" a change would defeat the whole feature.
            assert!(should_restart(None, Some("usb-interface-42")));
        }

        #[test]
        fn should_give_up_trips_at_exactly_the_configured_cap() {
            assert!(!should_give_up(MAX_CONSECUTIVE_FAILURES - 1));
            assert!(should_give_up(MAX_CONSECUTIVE_FAILURES));
            assert!(should_give_up(MAX_CONSECUTIVE_FAILURES + 1));
        }

        // ---- bookkeeping shape ----------------------------------------------

        #[test]
        fn a_fresh_follow_state_starts_fully_disarmed() {
            let state = DeviceFollowState::default();
            assert!(state.mic.armed.lock().unwrap().is_none());
            assert!(state.system.armed.lock().unwrap().is_none());
            assert_eq!(state.mic.consecutive_failures.load(Ordering::SeqCst), 0);
            assert_eq!(state.system.consecutive_failures.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn the_two_channels_bookkeeping_is_independent() {
            // Regression shape for a mistake that would be easy to make by
            // copy-pasting one channel's arm/disarm call sites into the
            // other's: tripping the mic's failure counter must never affect
            // system's, and vice versa — `channel.rs`'s
            // `one_channels_noise_floor_does_not_move_the_others` is the
            // same lesson for the audio pipeline.
            let state = DeviceFollowState::default();
            state
                .mic
                .consecutive_failures
                .fetch_add(1, Ordering::SeqCst);
            state.mic.restarting.store(true, Ordering::SeqCst);
            assert_eq!(state.mic.consecutive_failures.load(Ordering::SeqCst), 1);
            assert_eq!(state.system.consecutive_failures.load(Ordering::SeqCst), 0);
            assert!(!state.system.restarting.load(Ordering::SeqCst));
        }

        // ---- op lock: the fix for the manual-Stop / re-pin / silent-re-arm
        // races adversarial review found ---------------------------------
        //
        // `with_op_lock` itself needs a real `AppHandle` (it resolves
        // `DeviceFollowState` through Tauri's managed-state registry), which
        // is not available in a unit test — but it is a one-line wrapper
        // around `lock_op`, which takes the already-resolved bucket and is
        // exactly what these tests exercise directly. This is the actual
        // production primitive, not a re-implementation of it, so a
        // regression that made `lock_op` non-exclusive (or made a caller
        // stop holding it across a whole operation) is exactly what these
        // would catch — what they cannot catch is a *future* new call site
        // that mutates capture state without going through `with_op_lock`/
        // `lock_op` at all; that needs code review, not a unit test.

        #[test]
        fn the_op_lock_prevents_one_operations_steps_from_being_split_by_another() {
            // The literal shape of the races this fix closes:
            // `perform_restart` does stop-then-start as two separate steps
            // with a real device-open in between, and a manual
            // start/stop/re-pin command could previously run *between*
            // those two steps — reading `is_armed` in a state neither
            // "before the restart" nor "after it" genuinely represents.
            // Simulates that shape directly: two "operations," each logging
            // a step, sleeping (standing in for how long a real
            // `mic::start`/`system::start` call can block), then logging a
            // second step — and proves the log always shows one operation's
            // two steps back-to-back, never interrupted by the other's.
            let follow = Arc::new(FollowChannelState::default());
            let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

            let follow1 = follow.clone();
            let log1 = log.clone();
            let t1 = std::thread::spawn(move || {
                lock_op(&follow1, || {
                    log1.lock().unwrap().push("A: stop");
                    std::thread::sleep(std::time::Duration::from_millis(60));
                    log1.lock().unwrap().push("A: start");
                });
            });

            // Head start so t1 is reliably the one holding the lock when t2
            // tries — the assertion below only needs *a* deterministic
            // ordering to exist, not which operation goes first.
            std::thread::sleep(std::time::Duration::from_millis(15));

            let follow2 = follow.clone();
            let log2 = log.clone();
            let t2 = std::thread::spawn(move || {
                lock_op(&follow2, || {
                    log2.lock().unwrap().push("B: stop");
                    log2.lock().unwrap().push("B: start");
                });
            });

            t1.join().unwrap();
            t2.join().unwrap();

            let log = log.lock().unwrap();
            assert_eq!(
                *log,
                vec!["A: stop", "A: start", "B: stop", "B: start"],
                "operation B's steps landed inside operation A's stop-then-start \
                 window — exactly the interleaving that let a manual Stop or a \
                 manual re-pin race with an in-flight device-follow restart"
            );
        }

        #[test]
        fn the_op_lock_does_not_block_the_other_channel() {
            // Mic and system audio must stay independently controllable —
            // one channel's device-follow restart (which can block for as
            // long as the OS takes to open a device) must never stall the
            // other channel's manual start/stop.
            let state = Arc::new(DeviceFollowState::default());
            let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

            let state1 = state.clone();
            let log1 = log.clone();
            let t1 = std::thread::spawn(move || {
                lock_op(&state1.mic, || {
                    log1.lock().unwrap().push("mic: start");
                    std::thread::sleep(std::time::Duration::from_millis(60));
                    log1.lock().unwrap().push("mic: end");
                });
            });

            std::thread::sleep(std::time::Duration::from_millis(15));

            let state2 = state.clone();
            let log2 = log.clone();
            let t2 = std::thread::spawn(move || {
                lock_op(&state2.system, || {
                    log2.lock().unwrap().push("system: start");
                    log2.lock().unwrap().push("system: end");
                });
            });

            t1.join().unwrap();
            t2.join().unwrap();

            let log = log.lock().unwrap();
            let pos = |needle: &str| log.iter().position(|s| *s == needle).unwrap();
            // The system channel's whole operation (both its steps) must
            // complete *before* the still-running mic operation finishes,
            // proving it ran concurrently rather than queuing up behind the
            // mic lock — i.e. the two channels' op locks are genuinely
            // separate mutexes, not one lock shared across the whole app.
            assert!(
                pos("system: start") < pos("mic: end") && pos("system: end") < pos("mic: end"),
                "the system channel's operation was blocked by the mic \
                 channel's in-flight operation — the op lock must be \
                 per-channel, log: {log:?}"
            );
        }
    }
}

pub use device_follow::DeviceFollowState;

/// Disarms auto-follow for both channels. Called once at app shutdown
/// alongside the existing mic/system-audio teardown — see `lib.rs`'s
/// `is_shutdown_event` handling. Without this, a CoreAudio listener could
/// still be registered (and, in a genuine race, could still fire and spawn a
/// restart thread) after the app has started tearing everything else down.
pub fn disarm_device_follow(app: &AppHandle) {
    device_follow::disarm_all(app);
}
