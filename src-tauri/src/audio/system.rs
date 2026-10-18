//! System-audio capture — hearing the other people on the call.
//!
//! Aside's microphone path only ever hears *you*. This module captures what
//! comes **out** of the speakers, which is everyone else. Together they give
//! the two separately-labelled channels the rest of the app is built around.
//!
//! ## How it works, in one paragraph
//!
//! Both macOS and Windows expose a "record what's playing" mode, and `cpal`
//! reaches it the same way on both: you take an **output** device and ask it
//! for an *input* stream. On macOS `cpal` builds a CoreAudio Process Tap plus
//! a private aggregate device behind the scenes; on Windows it sets the WASAPI
//! `LOOPBACK` flag. From our side it is one code path, which is the entire
//! reason `cpal` was upgraded from 0.15 to 0.18 for this phase — 0.15 had the
//! Windows half only, and hand-writing the macOS half meant ~350 lines of
//! unsafe CoreAudio glue that `cpal` 0.18 had already written, debugged, and
//! shipped three bugfixes for.
//!
//! ## What this module has to add on top of `cpal`
//!
//! Three things `cpal` deliberately does not do, each of which is a silent
//! failure if skipped — and silent failures are the defining hazard of this
//! whole API surface:
//!
//! 1. **A macOS version gate.** `cpal` 0.18 calls the tap selectors with no
//!    availability check anywhere in the crate (verified by grepping it). On
//!    macOS older than the tap API, that is an Objective-C
//!    `unrecognized selector` raise, which aborts the process rather than
//!    returning an `Err` — so a `match ... { Err(_) => fallback }` never runs.
//!    We check the OS version *before* handing control to `cpal`.
//! 2. **Refusing devices that are not actually tappable.** `cpal` only builds
//!    a tap when the device does not itself support input. Hand it a device
//!    that does — virtual audio devices like "Microsoft Teams Audio", which
//!    really is present on this developer machine — and it silently opens that
//!    device's *input* side instead. You get audio, no error, and it is the
//!    wrong audio.
//! 3. **Detecting the permission denial that isn't reported.** macOS returns
//!    success and then feeds zero-filled buffers when system-audio recording
//!    is not permitted. There is no error code to check. See [`SilenceWatch`].
//!
//! ## Not how screen-share hiding works
//!
//! Capturing system audio is what any recorder does and hides nothing from
//! anyone. Screen-share hiding lives in `crate::stealth` (window flags), not
//! here. The one flag in this file that *sounds* like concealment, `cpal`'s
//! internal `setPrivate(true)` on the aggregate device, means "don't publish
//! this into the global CoreAudio device list" — it is resource hygiene that
//! stops orphaned devices piling up in Audio MIDI Setup after a crash, and
//! conceals nothing from call participants.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::aec::AecHub;
use super::channel::{Channel, ChannelPipeline, StreamRoute};
use super::device_util::{device_id, device_name, same_physical_device};

/// Where macOS records its own version number. Reading this file avoids both a
/// subprocess spawn (`sw_vers`) and an Objective-C dependency pulled in purely
/// to ask one question.
#[cfg(target_os = "macos")]
const SYSTEM_VERSION_PLIST: &str = "/System/Library/CoreServices/SystemVersion.plist";

/// The oldest macOS `cpal` 0.18 claims working loopback on.
///
/// Its changelog says "macOS > 14.6"; the underlying Process Tap API arrived
/// in 14.4 but `cpal`'s own support is only asserted above 14.6, so 14.7 is
/// the honest floor. Refusing on 14.4–14.6 gives a clear message; guessing and
/// being wrong gives zero-filled buffers or a process abort.
#[cfg(target_os = "macos")]
const MIN_MACOS: (u32, u32, u32) = (14, 7, 0);

/// An output device that could be used as a capture source.
#[derive(Debug, Clone, Serialize)]
pub struct OutputDevice {
    /// Stable identifier — this is what gets saved and passed back to
    /// `start_system_audio_capture`, never the display name.
    pub id: Option<String>,
    pub name: String,
    pub is_default: bool,
    /// False when this device also exposes an input side.
    ///
    /// Surfaced rather than silently filtered because "why isn't my Teams
    /// virtual device in the list" is a question worth being able to answer.
    pub tappable: bool,
}

/// What actually happened when capture started. Reported to the frontend so
/// "which path ran, at what rate" is observable rather than guessed at —
/// especially on Windows, which ships without ever having been run.
///
/// `device_name`/`device_id` were added after a real debugging session where
/// the only question that mattered — *which* device was actually being tapped
/// — was the one thing the app could not answer. On this machine the default
/// output resolves to a Bluetooth headset whose CoreAudio id ends in
/// `:output`, and that suffix is exactly what distinguishes it from the same
/// headset's microphone; without it in the UI, "AirPods" is ambiguous between
/// two different CoreAudio devices.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub device_name: String,
    pub device_id: Option<String>,
}

/// Watches for the failure mode this API has no error code for.
///
/// When macOS denies system-audio recording, every CoreAudio call still
/// returns success and the buffers simply arrive full of zeros. A missing
/// `NSAudioCaptureUsageDescription` produces exactly the same silence with no
/// prompt at all — which is precisely the bug that already cost this project a
/// debugging session on the microphone, where macOS silently muted input
/// instead of asking.
///
/// **What this can and cannot tell you.** A *granted* tap also delivers zeros
/// whenever nothing is playing, so silence is genuinely ambiguous: it means
/// "denied" or "nobody is talking", and no amount of inspecting the samples
/// separates them. That is why the window is long and why the result is
/// phrased as a hint on the `notice` channel rather than an error. An earlier
/// 12-second window would have fired on any call that opened with a quiet
/// stretch, painting a red "check your permissions" banner over a session that
/// was working fine.
///
/// **Peak-to-peak, not `abs(sample)`.** Some hardware reports a small constant
/// DC offset rather than true zero. An `abs()` test latches on that offset,
/// concludes audio is flowing, and permanently disables the very warning it
/// exists to produce. `max - min` is invariant to any constant offset, so a
/// biased-but-silent stream still reads as silent.
///
/// **Accumulated across calls, not reset per buffer.** An adversarial review
/// found that computing peak-to-peak fresh inside each `observe` call misses
/// real signal that never shows both a peak and a trough *within one buffer*
/// — a single-sample buffer always has `hi == lo` no matter its value, and
/// even an ordinary-sized buffer can catch a slowly-varying signal at a
/// moment its swing within that one window is small. `running_lo`/`running_hi`
/// track the true extremes seen since the last confirmed signal, so a buffer
/// that alone looks flat still contributes to detecting real audio a few
/// callbacks later.
#[derive(Debug)]
pub struct SilenceWatch {
    seen_signal: bool,
    silent_ms: f32,
    warned: bool,
    threshold: f32,
    warn_after_ms: f32,
    running_lo: f32,
    running_hi: f32,
}

impl Default for SilenceWatch {
    fn default() -> Self {
        Self {
            seen_signal: false,
            silent_ms: 0.0,
            warned: false,
            // Comfortably above the ±1-2 LSB dither of a real-but-quiet
            // stream, far below anything audible.
            threshold: 1e-4,
            // A full minute. Long enough that joining a call early, or a
            // genuinely quiet stretch, never triggers it; short enough to
            // still catch a misconfigured permission well inside a meeting.
            warn_after_ms: 60_000.0,
            running_lo: f32::INFINITY,
            running_hi: f32::NEG_INFINITY,
        }
    }
}

impl SilenceWatch {
    /// Feeds one buffer in. Returns `true` exactly once, on the callback where
    /// the stream has been silent long enough to be worth mentioning.
    ///
    /// Only fires while `seen_signal` is false. Once real audio has been
    /// observed the permission question is settled, and later silence just
    /// means nobody is talking — which is not a fault.
    pub fn observe(&mut self, mono: &[f32], elapsed_ms: f32) -> bool {
        // Non-finite samples are not audio. Skipping them here (rather than
        // bailing out of the whole buffer) means a device emitting NaN or
        // infinity still counts as silent and still accrues elapsed time —
        // an earlier version returned early and so could have its warning
        // deferred indefinitely by a single bad sample.
        for &s in mono {
            if s.is_finite() {
                self.running_lo = self.running_lo.min(s);
                self.running_hi = self.running_hi.max(s);
            }
        }
        let has_signal = self.running_hi.is_finite()
            && self.running_lo.is_finite()
            && (self.running_hi - self.running_lo) > self.threshold;

        if has_signal {
            self.seen_signal = true;
            self.silent_ms = 0.0;
            return false;
        }
        if self.seen_signal || self.warned {
            return false;
        }
        self.silent_ms += elapsed_ms;
        if self.silent_ms >= self.warn_after_ms {
            self.warned = true;
            return true;
        }
        false
    }
}

/// Pulls `ProductVersion` out of macOS's SystemVersion.plist.
///
/// A deliberately tiny parser rather than a plist crate: this reads one
/// well-known key from one file whose format has been stable for two decades.
/// Pure, so the version gate is testable without owning machines running every
/// macOS release.
pub fn parse_product_version(plist: &str) -> Option<(u32, u32, u32)> {
    let key = plist.find("<key>ProductVersion</key>")?;
    let rest = &plist[key..];
    let open = rest.find("<string>")? + "<string>".len();
    let close = rest.find("</string>")?;
    if close < open {
        return None;
    }
    parse_version_triple(rest[open..close].trim())
}

/// Parses `"15.3.1"`, `"14.7"` or `"27"` into a comparable triple.
pub fn parse_version_triple(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    // A missing component means zero: "14.7" is 14.7.0.
    let minor = parts
        .next()
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// Ordinary tuple ordering, named so the comparison direction is unambiguous
/// at the call site.
pub fn meets_minimum(actual: (u32, u32, u32), minimum: (u32, u32, u32)) -> bool {
    actual >= minimum
}

/// Refuses to continue on a macOS too old for `cpal`'s loopback support.
///
/// This must run *before* any `cpal` loopback call. `cpal` 0.18 has no
/// availability check of its own, and the failure it is guarding against is a
/// process abort from the Objective-C runtime, not a catchable error.
#[cfg(target_os = "macos")]
fn check_platform_support() -> Result<(), String> {
    let plist = std::fs::read_to_string(SYSTEM_VERSION_PLIST)
        .map_err(|e| format!("could not read macOS version from {SYSTEM_VERSION_PLIST}: {e}"))?;
    let version = parse_product_version(&plist)
        .ok_or_else(|| "could not parse ProductVersion from SystemVersion.plist".to_string())?;
    if !meets_minimum(version, MIN_MACOS) {
        return Err(format!(
            "System audio capture needs macOS {}.{} or newer (this machine is {}.{}.{}). \
             Your microphone still works — only hearing the other participants is unavailable.",
            MIN_MACOS.0, MIN_MACOS.1, version.0, version.1, version.2
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn check_platform_support() -> Result<(), String> {
    // WASAPI loopback has no version floor worth gating on — it predates every
    // Windows version Aside could plausibly run on.
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn check_platform_support() -> Result<(), String> {
    Err("System audio capture is only implemented for macOS and Windows.".to_string())
}

/// True when `cpal` will build a loopback tap for this device rather than
/// opening its input side.
///
/// This calls the *same* predicate cpal branches on internally
/// (`if self.supports_input() { plain input } else { loopback }`), rather than
/// approximating it — an earlier version used `default_input_config().is_err()`
/// as a proxy, which diverges on macOS for any device whose input format is
/// not linear-PCM f32/i16: cpal would take the plain-input path while the
/// proxy said "tappable", producing confident capture from the wrong source.
fn is_tappable(device: &cpal::Device) -> bool {
    !device.supports_input()
}

/// Lists output devices that could serve as a capture source.
pub fn list_output_devices() -> Result<Vec<OutputDevice>, String> {
    let host = cpal::default_host();
    // Compared by id, not name: two devices can share a name, in which case a
    // name comparison would mark both as the default.
    let default_id = host.default_output_device().and_then(|d| device_id(&d));
    let devices = host
        .output_devices()
        .map_err(|e| format!("could not enumerate output devices: {e}"))?;

    Ok(devices
        .map(|d| {
            let id = device_id(&d);
            OutputDevice {
                is_default: id.is_some() && id == default_id,
                tappable: is_tappable(&d),
                name: device_name(&d),
                id,
            }
        })
        .collect())
}

/// Picks the output device to capture from, by stable id, or the system default.
pub(super) fn resolve_device(preferred: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    if let Some(want) = preferred {
        // A saved device id can outlive the device — unplugged, or carried
        // over from another machine. Fall back rather than fail hard.
        let found = host
            .output_devices()
            .map_err(|e| format!("could not enumerate output devices: {e}"))?
            .find(|d| device_id(d).as_deref() == Some(want));
        if let Some(d) = found {
            return Ok(d);
        }
        eprintln!("system audio: saved output device {want:?} not found, using the default");
    }

    host.default_output_device()
        .ok_or_else(|| "no audio output device found".to_string())
}

/// A running system-audio capture. Dropping or `stop`ping it tears the
/// stream down.
///
/// The `Drop` impl is the load-bearing half, exactly as in `MicCaptureHandle`:
/// any path that loses the handle without an explicit `stop()` — a panic, an
/// early return, a poisoned `Mutex` — would otherwise leave the capture thread
/// alive with a CoreAudio tap and an aggregate device still held, and no way
/// left to reach it.
pub struct SystemCaptureHandle {
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub info: CaptureInfo,
}

impl SystemCaptureHandle {
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for SystemCaptureHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Starts capturing system output audio on a dedicated OS thread.
///
/// True when a live cpal stream error means "re-open the tap", not "give up".
///
/// macOS reports `Device sample rate changed` when the output mix format
/// flips (a call starting, Bluetooth HFP, an aggregate device reconfiguring).
/// cpal 0.18 surfaces that as `BackendSpecific` with that Display text, not a
/// dedicated variant — matching the string is the only portable check.
/// The Speechmatics engine already resamples whatever rate arrives; the tap
/// itself is what has to be rebuilt.
pub fn is_recoverable_stream_error(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("sample rate") || m.contains("sample-rate")
}

/// Blocks until the stream is confirmed running or has failed, so callers get
/// an honest success or error rather than discovering later that no audio ever
/// arrived. Never degrades to a silent stream that produces nothing — that
/// failure mode is the whole reason this module is careful.
pub fn start(
    app: AppHandle,
    preferred_device: Option<String>,
    stt_route: StreamRoute,
    aec: AecHub,
) -> Result<SystemCaptureHandle, String> {
    check_platform_support()?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop = stop_flag.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<CaptureInfo, String>>();

    let thread = std::thread::spawn(move || {
        let device = match resolve_device(preferred_device.as_deref()) {
            Ok(d) => d,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let name = device_name(&device);

        // Refuse rather than capture the wrong thing. A device with an input
        // side gets opened as a plain input by cpal, with no tap and no error.
        if !is_tappable(&device) {
            let _ = ready_tx.send(Err(format!(
                "{name:?} exposes its own input, so the system cannot tap what it plays. \
                 Pick a different output device (your speakers or headphones)."
            )));
            return;
        }

        let this_id = device_id(&device);

        // Warned about, not refused. macOS cannot tap a device while that same
        // device is also the active microphone — the tap starts, reports
        // success, and every buffer is zeros. But this is only actually fatal
        // when the mic is genuinely open: this same headset taps perfectly
        // when it is not, which was confirmed live on this machine. Refusing
        // outright would block a working configuration, so the honest move is
        // to say so immediately rather than let `SilenceWatch` shrug about it
        // sixty seconds later.
        if same_physical_device(
            this_id.as_deref(),
            cpal::default_host()
                .default_input_device()
                .and_then(|d| device_id(&d))
                .as_deref(),
        ) {
            let _ = app.emit(
                Channel::System.notice_event(),
                format!(
                    "{name:?} is currently both your output and your microphone. macOS cannot \
                     capture a device while it is also the active mic — if this channel stays \
                     silent, switch your input to the built-in microphone (System Settings > \
                     Sound > Input) and start it again."
                ),
            );
        }

        // Re-open the tap when macOS changes the mix format. The STT engine
        // already resamples; the dead stream is the thing that must be rebuilt.
        let mut announced = false;
        const NO_CALLBACK_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
        loop {
            if thread_stop.load(Ordering::SeqCst) {
                break;
            }

            // A render endpoint's *input* config is meaningless — on Windows it is
            // an error and the supported-input list is empty. The output config is
            // the mix format, and it is what the loopback stream must match.
            let config = match device.default_output_config() {
                Ok(c) => c,
                Err(e) => {
                    let msg = format!("no output config for {name:?}: {e}");
                    if !announced {
                        let _ = ready_tx.send(Err(msg));
                        return;
                    }
                    let _ = app.emit(Channel::System.error_event(), msg);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            let sample_format = config.sample_format();
            let stream_config: cpal::StreamConfig = config.into();
            let sample_rate = stream_config.sample_rate;
            // Interleaved samples: the channel count is required to convert a
            // buffer length into real elapsed time. Output devices are routinely
            // stereo — on this developer machine every one of them is — so unlike
            // the mic path, the multi-channel branch here is the *normal* case.
            let channels = stream_config.channels;
            let info = CaptureInfo {
                sample_rate,
                channels,
                device_name: name.clone(),
                device_id: this_id.clone(),
            };

            let callbacks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let format_changed = Arc::new(AtomicBool::new(false));
            let mut pipeline = ChannelPipeline::new(Channel::System);
            pipeline.set_stt_filter(Arc::new(aec.clone()));
            let initial_status = pipeline.status();
            let mut watch = SilenceWatch::default();

            let err_app = app.clone();
            let err_changed = format_changed.clone();
            let err_fn = move |e: cpal::Error| {
                let text = e.to_string();
                eprintln!("system audio stream error: {text}");
                if is_recoverable_stream_error(&text) {
                    err_changed.store(true, Ordering::SeqCst);
                    let _ = err_app.emit(
                        Channel::System.notice_event(),
                        "System audio format changed; reconnecting the tap.".to_string(),
                    );
                } else {
                    let _ = err_app.emit(Channel::System.error_event(), text);
                }
            };

            let warn_app = app.clone();
            let cb_count = callbacks.clone();
            let check_silence = move |mono: &[f32], watch: &mut SilenceWatch| {
                cb_count.fetch_add(1, Ordering::Relaxed);
                let elapsed_ms = (mono.len() as f32 / sample_rate as f32) * 1000.0;
                if watch.observe(mono, elapsed_ms) {
                    let _ = warn_app.emit(
                        Channel::System.notice_event(),
                        "No audio heard from this device yet. If nobody has spoken, this is \
                         normal. Otherwise: check Aside is allowed to record system audio in \
                         System Settings > Privacy & Security, and note that macOS cannot \
                         capture a device while it is also your active microphone — if you are \
                         on AirPods, switch your input to the built-in microphone."
                            .to_string(),
                    );
                }
            };

            let stream = match sample_format {
                cpal::SampleFormat::F32 => {
                    let app = app.clone();
                    let stt_route = stt_route.clone();
                    device.build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let stt_tx = stt_route.try_lock().ok().and_then(|g| g.clone());
                            let mono = pipeline.process_f32(
                                &app,
                                data,
                                channels,
                                sample_rate,
                                stt_tx.as_ref(),
                            );
                            check_silence(mono, &mut watch);
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let app = app.clone();
                    let stt_route = stt_route.clone();
                    device.build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let stt_tx = stt_route.try_lock().ok().and_then(|g| g.clone());
                            let mono = pipeline.process_i16(
                                &app,
                                data,
                                channels,
                                sample_rate,
                                stt_tx.as_ref(),
                            );
                            check_silence(mono, &mut watch);
                        },
                        err_fn,
                        None,
                    )
                }
                other => {
                    let msg = format!(
                        "unsupported system audio sample format: {other:?} (only f32/i16 are handled today)"
                    );
                    if !announced {
                        let _ = ready_tx.send(Err(msg));
                        return;
                    }
                    let _ = app.emit(Channel::System.error_event(), msg);
                    break;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let msg = format!("failed to build system audio stream on {name:?}: {e}");
                    if !announced {
                        let _ = ready_tx.send(Err(msg));
                        return;
                    }
                    let _ = app.emit(Channel::System.error_event(), msg);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            if let Err(e) = stream.play() {
                let msg = format!("failed to start system audio stream: {e}");
                if !announced {
                    let _ = ready_tx.send(Err(msg));
                    return;
                }
                let _ = app.emit(Channel::System.error_event(), msg);
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }

            let _ = app.emit(Channel::System.vad_state_event(), initial_status);
            if !announced {
                let _ = ready_tx.send(Ok(info));
                announced = true;
            }

            let started = std::time::Instant::now();
            let mut no_callback_warned = false;
            while !thread_stop.load(Ordering::SeqCst) && !format_changed.load(Ordering::SeqCst) {
                if !no_callback_warned
                    && started.elapsed() >= NO_CALLBACK_GRACE
                    && callbacks.load(Ordering::Relaxed) == 0
                {
                    no_callback_warned = true;
                    let _ = app.emit(
                        Channel::System.notice_event(),
                        format!(
                            "The capture stream on {name:?} started but macOS has delivered no audio \
                             buffers at all in {}s. That is different from capturing silence — it \
                             usually means the tap was created but never fed. Try quitting and \
                             reopening Aside (a permission granted while it was already running does \
                             not reach the running process), or pick a different output device.",
                            NO_CALLBACK_GRACE.as_secs()
                        ),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            // `stream` drops here, which stops capture and — on macOS — destroys
            // the aggregate device and process tap cpal created for it.
            if thread_stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });

    let info = super::device_util::wait_for_capture_start(
        &ready_rx,
        &stop_flag,
        std::time::Duration::from_secs(8),
        "System audio",
    )?;
    Ok(SystemCaptureHandle {
        stop_flag,
        thread: Some(thread),
        info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_change_is_a_recoverable_stream_error() {
        assert!(is_recoverable_stream_error("Device sample rate changed"));
        assert!(is_recoverable_stream_error(
            "The device sample rate has changed"
        ));
        assert!(!is_recoverable_stream_error(
            "failed to build system audio stream"
        ));
        assert!(!is_recoverable_stream_error("permission denied"));
    }

    #[test]
    fn capture_info_json_uses_the_snake_case_keys_the_inspector_reads() {
        let info = CaptureInfo {
            sample_rate: 48_000,
            channels: 2,
            device_name: "MacBook Air Speakers".into(),
            device_id: Some("BuiltInSpeakerDevice:output".into()),
        };
        let v = serde_json::to_value(&info).expect("CaptureInfo should serialize");
        assert_eq!(v["sample_rate"], 48_000);
        assert_eq!(v["channels"], 2);
        assert_eq!(v["device_name"], "MacBook Air Speakers");
        assert_eq!(v["device_id"], "BuiltInSpeakerDevice:output");
        assert!(
            v.get("sampleRate").is_none(),
            "camelCase would silently miss the inspector's field names"
        );
    }

    // ---- version parsing -------------------------------------------------

    #[test]
    fn parses_a_real_system_version_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>ProductBuildVersion</key>
	<string>26A5416b</string>
	<key>ProductName</key>
	<string>macOS</string>
	<key>ProductVersion</key>
	<string>27.0</string>
</dict>
</plist>"#;
        assert_eq!(parse_product_version(plist), Some((27, 0, 0)));
    }

    #[test]
    fn does_not_confuse_product_version_with_the_keys_around_it() {
        // ProductBuildVersion and ProductName both appear before it and both
        // have <string> values. A naive "first <string> after any Product key"
        // parser would return the build number.
        let plist = "<key>ProductBuildVersion</key><string>26A5416b</string>\
                     <key>ProductVersion</key><string>15.3.1</string>";
        assert_eq!(parse_product_version(plist), Some((15, 3, 1)));
    }

    #[test]
    fn missing_components_read_as_zero() {
        assert_eq!(parse_version_triple("14.7"), Some((14, 7, 0)));
        assert_eq!(parse_version_triple("27"), Some((27, 0, 0)));
        assert_eq!(parse_version_triple("15.3.1"), Some((15, 3, 1)));
    }

    #[test]
    fn malformed_versions_are_rejected_rather_than_guessed() {
        assert_eq!(parse_version_triple(""), None);
        assert_eq!(parse_version_triple("sonoma"), None);
        assert_eq!(parse_product_version("<plist></plist>"), None);
    }

    #[test]
    fn the_version_gate_admits_and_refuses_the_right_releases() {
        let min = (14, 7, 0);
        // Refused: the Process Tap API exists from 14.4 but cpal only claims
        // loopback above 14.6, and guessing wrong here is silence or a crash.
        assert!(!meets_minimum((14, 4, 0), min));
        assert!(!meets_minimum((14, 6, 9), min));
        assert!(!meets_minimum((13, 0, 0), min));
        // Admitted.
        assert!(meets_minimum((14, 7, 0), min));
        assert!(meets_minimum((15, 0, 0), min));
        assert!(meets_minimum((27, 0, 0), min));
    }

    #[test]
    fn version_comparison_is_numeric_not_lexicographic() {
        // "9" > "27" as strings. A string comparison would refuse macOS 27
        // while admitting macOS 9, so pin the numeric ordering.
        assert!(meets_minimum((27, 0, 0), (9, 0, 0)));
        assert!(!meets_minimum((9, 0, 0), (27, 0, 0)));
        // Minor versions likewise: 14.10 is newer than 14.7.
        assert!(meets_minimum((14, 10, 0), (14, 7, 0)));
    }

    #[test]
    fn this_machine_passes_its_own_gate() {
        // Guards the plist path and the parser against a real file rather than
        // a fixture. If Apple ever moves or reformats it, this fails loudly
        // here instead of silently disabling system audio for everyone.
        #[cfg(target_os = "macos")]
        {
            let plist = std::fs::read_to_string(SYSTEM_VERSION_PLIST)
                .expect("SystemVersion.plist should be readable on macOS");
            let v = parse_product_version(&plist).expect("ProductVersion should parse");
            assert!(v.0 >= 10, "implausible macOS major version {v:?}");
            assert!(
                check_platform_support().is_ok(),
                "this machine ({v:?}) should satisfy the {MIN_MACOS:?} floor"
            );
        }
    }

    // ---- silent-permission detection -------------------------------------
    //
    // `same_physical_device`/`strip_device_role_suffix`'s own tests moved to
    // `device_util.rs` alongside the functions themselves — see this
    // phase's move to share them with `device_watch`'s restart-suppression
    // guard.

    fn tone(n: usize, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    /// Number of 10 ms buffers needed to just clear `warn_after_ms`. Derived
    /// from the field itself, not a hardcoded count — a hardcoded loop count
    /// is exactly what silently desynced from `warn_after_ms` before (the
    /// window moved from 12 s to 60 s and three tests kept looping to 20 s).
    fn iters_to_clear(w: &SilenceWatch) -> usize {
        (w.warn_after_ms / 10.0).ceil() as usize + 1
    }

    #[test]
    fn warns_once_after_a_long_stretch_of_digital_silence() {
        let mut w = SilenceWatch::default();
        let silent = vec![0.0_f32; 480];
        let mut warnings = 0;
        for _ in 0..iters_to_clear(&w) {
            if w.observe(&silent, 10.0) {
                warnings += 1;
            }
        }
        assert_eq!(warnings, 1, "should warn exactly once, not every callback");
        assert!(!w.seen_signal);
    }

    #[test]
    fn does_not_warn_before_the_grace_period() {
        let mut w = SilenceWatch::default();
        let silent = vec![0.0_f32; 480];
        // One buffer short of the threshold — a meeting can easily open with
        // a long quiet stretch before anyone speaks, and this must not warn
        // during it.
        let short_by_one = iters_to_clear(&w) - 2;
        for _ in 0..short_by_one {
            assert!(!w.observe(&silent, 10.0));
        }
    }

    #[test]
    fn real_audio_cancels_the_warning_permanently() {
        let mut w = SilenceWatch::default();
        let silent = vec![0.0_f32; 480];
        for _ in 0..500 {
            w.observe(&silent, 10.0);
        }
        assert!(w.observe(&tone(480, 0.2), 10.0) == false);
        assert!(w.seen_signal);
        // Once audio has been proven to flow, permission is settled. Later
        // silence just means nobody is talking, which is not a fault.
        let mut warnings = 0;
        for _ in 0..5_000 {
            if w.observe(&silent, 10.0) {
                warnings += 1;
            }
        }
        assert_eq!(warnings, 0);
    }

    #[test]
    fn a_dc_offset_does_not_masquerade_as_signal() {
        // THE regression test for this detector. Some hardware reports a
        // constant offset instead of true zero. A detector written as
        // `abs(sample) > threshold` sees 0.02 and concludes audio is flowing,
        // permanently disabling the warning it exists to raise.
        let mut w = SilenceWatch::default();
        let biased = vec![0.02_f32; 480];

        // Guard: the naive test really would be fooled by this input, so this
        // test is meaningless if that stops being true.
        assert!(
            biased.iter().all(|s| s.abs() > 1e-4),
            "fixture must exceed an abs() threshold or this proves nothing"
        );

        let mut warnings = 0;
        for _ in 0..iters_to_clear(&w) {
            if w.observe(&biased, 10.0) {
                warnings += 1;
            }
        }
        assert!(
            !w.seen_signal,
            "a constant offset carries no audio and must not count as signal"
        );
        assert_eq!(warnings, 1, "the warning must still fire through DC bias");
    }

    #[test]
    fn a_very_quiet_but_real_signal_still_counts_as_audio() {
        // The mirror of the DC-bias test: the threshold must not be so high
        // that genuinely quiet speech reads as a permission failure.
        let mut w = SilenceWatch::default();
        assert!(!w.observe(&tone(480, 0.001), 10.0));
        assert!(w.seen_signal);
    }

    #[test]
    fn signal_split_across_separate_calls_is_still_detected() {
        // THE regression test for the running-accumulator fix. Peak-to-peak
        // computed fresh inside one call alone misses this: a single-sample
        // buffer always has `hi == lo` no matter its value, so real audio
        // arriving one extreme per call never showed a peak *and* a trough in
        // the same call. An adversarial review reproduced this as a false
        // "permission denied" on genuinely live audio.
        let mut w = SilenceWatch::default();
        assert!(!w.observe(&[0.5], 1.0));
        assert!(!w.seen_signal, "one extreme alone proves nothing yet");
        assert!(!w.observe(&[-0.5], 1.0));
        assert!(
            w.seen_signal,
            "the two extremes together span well past the threshold and must register as signal"
        );
    }

    #[test]
    fn one_nan_frame_does_not_permanently_disable_the_detector() {
        // vad.rs was bitten by exactly this: a single NaN killed adaptation
        // for the rest of the session. NaN poisons min/max comparisons, so it
        // is skipped rather than allowed to wedge the bounds.
        let mut w = SilenceWatch::default();
        let mut buf = vec![0.0_f32; 480];
        buf[7] = f32::NAN;
        let mut warnings = 0;
        for _ in 0..iters_to_clear(&w) {
            if w.observe(&buf, 10.0) {
                warnings += 1;
            }
        }
        assert_eq!(warnings, 1, "NaN frames must not disable the warning");
    }

    #[test]
    fn an_all_nan_buffer_still_accrues_toward_the_silence_warning() {
        // An all-NaN buffer carries no finite samples at all, so it can never
        // look like signal — but per `observe`'s doc comment it must still
        // accrue elapsed time exactly like real silence, not be skipped
        // outright (an earlier version bailed out on a bad sample and could
        // defer its warning indefinitely). The previous version of this test
        // only ran for 50s of a 60s window, so it passed whether or not that
        // held — confirmed by patching `observe` to skip NaN-only buffers
        // entirely and finding the full suite, including this test, still
        // green.
        let mut w = SilenceWatch::default();
        let nans = vec![f32::NAN; 480];
        let mut warnings = 0;
        for _ in 0..iters_to_clear(&w) {
            if w.observe(&nans, 10.0) {
                warnings += 1;
            }
        }
        assert_eq!(
            warnings, 1,
            "an all-NaN buffer must still accrue silence and eventually warn"
        );
        assert!(
            !w.seen_signal,
            "NaN alone must never be classified as real signal"
        );
    }

    #[test]
    fn empty_buffers_are_ignored() {
        let mut w = SilenceWatch::default();
        for _ in 0..5_000 {
            assert!(!w.observe(&[], 10.0));
        }
    }

    /// Live hardware probe for the gap `status.md` 3.8 records: every other
    /// test in this file is pure logic, and **no test has ever actually
    /// opened a system-audio stream on real hardware.** This one does.
    ///
    /// It deliberately reuses this module's own `resolve_device` /
    /// `is_tappable` / `device_name` rather than re-deriving them, because
    /// picking the wrong device is the most likely way this feature fails
    /// silently — see the module doc comment's point 2.
    ///
    /// Asserts only what is true regardless of whether anything is playing:
    /// the platform gate passes, a device resolves, our tappability guard
    /// agrees with cpal's own behaviour, and the stream opens and runs. It
    /// **prints** the peak amplitude seen, which is the number that tells a
    /// human whether the tap is really delivering audio — see
    /// `a_playing_system_audio_stream_delivers_non_silent_audio` below for
    /// the assertion version.
    ///
    /// Run: `cargo test --lib audio::system::tests::a_real_system_audio_stream -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn a_real_system_audio_stream_opens_on_this_machine() {
        let _ = probe_system_audio(3);
    }

    /// The assertion half of the probe above. **Only meaningful while audio
    /// is actually playing out of the selected output device** — run it with
    /// music or a video playing, otherwise silence is the correct result and
    /// this failing tells you nothing. Separate from the test above precisely
    /// so the unconditional checks are never skipped just because nothing
    /// happened to be playing.
    ///
    /// Run: `cargo test --lib audio::system::tests::a_playing_system_audio -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn a_playing_system_audio_stream_delivers_non_silent_audio() {
        let peak = probe_system_audio(5);
        assert!(
            peak > 0.0005,
            "captured 5s of effectively-silent system audio (peak {peak:.6}). \
             If audio really was playing, this is the silent-failure case: check \
             System Settings > Privacy & Security for Aside's system-audio permission, \
             and note macOS cannot tap a device that is simultaneously the active \
             microphone (AirPods are the usual culprit — switch input to the built-in mic)."
        );
    }

    /// Diagnostic, not an assertion — prints the real device ids cpal reports
    /// on this machine. Kept (rather than deleted after it did its job of
    /// grounding `same_physical_device` in what cpal actually returns) because
    /// "which CoreAudio device is which" is the first question every future
    /// system-audio bug will need answered, and guessing at it is what made
    /// this one take a session.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn print_this_machines_real_device_ids() {
        let host = cpal::default_host();
        if let Some(d) = host.default_input_device() {
            println!(
                "default INPUT  name={:?} id={:?}",
                device_name(&d),
                device_id(&d)
            );
        }
        if let Some(d) = host.default_output_device() {
            println!(
                "default OUTPUT name={:?} id={:?}",
                device_name(&d),
                device_id(&d)
            );
        }
        for d in host.output_devices().unwrap() {
            println!(
                "  out: name={:?} id={:?} tappable={}",
                device_name(&d),
                device_id(&d),
                is_tappable(&d)
            );
        }
    }

    /// The specific failure the owner actually hit, isolated: capture system
    /// audio **while the same device is also serving as the active
    /// microphone**. On Bluetooth headsets (AirPods) macOS switches the link
    /// into headset mode when the mic opens, which changes the output clock —
    /// and is the case this module's own `system-notice` text already warns
    /// about. Aside runs exactly this combination whenever both channels are
    /// live, so "works with the mic closed" is not evidence it works in use.
    ///
    /// Like the test above, only meaningful **while audio is playing**. It
    /// asserts nothing about the outcome — it reports it — because both
    /// results are informative and neither is a code defect: the point is to
    /// produce the number, not to pass.
    ///
    /// Run: `cargo test --lib audio::system::tests::system_audio_while_the_mic -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn system_audio_while_the_mic_is_open_on_the_same_device() {
        let host = cpal::default_host();
        let input = host
            .default_input_device()
            .expect("a default input device should exist");
        println!("contention probe: input device = {:?}", device_name(&input));
        let in_config = input
            .default_input_config()
            .expect("default input config")
            .config();
        let mic = input
            .build_input_stream(
                in_config,
                move |_: &[f32], _: &cpal::InputCallbackInfo| {},
                |e: cpal::Error| eprintln!("mic probe stream error: {e}"),
                None,
            )
            .expect("mic stream should build");
        mic.play().expect("mic stream should start");
        // Let macOS settle into headset mode before the tap opens — the
        // reconfiguration is not instant, and opening the tap mid-switch
        // would measure the transition rather than the steady state.
        std::thread::sleep(std::time::Duration::from_millis(1500));

        let peak = probe_system_audio(5);
        drop(mic);
        println!(
            "contention probe verdict: system-audio peak with the mic open = {peak:.6} \
             ({})",
            if peak > 0.0005 {
                "AUDIO PRESENT"
            } else {
                "SILENT"
            }
        );
    }

    /// Shared body of the two probes: resolves the same device the app would,
    /// runs the same guards, opens a real capture stream for `seconds`, and
    /// returns the largest absolute sample seen. Panics with the app's own
    /// error text on any failure, so a probe failure reads the same as a real
    /// user-facing failure would.
    #[cfg(target_os = "macos")]
    fn probe_system_audio(seconds: u64) -> f32 {
        check_platform_support().expect("this machine should pass the macOS version gate");

        let device = resolve_device(None).expect("a default output device should resolve");
        let name = device_name(&device);
        println!(
            "system-audio probe: device = {name:?}, tappable = {}",
            is_tappable(&device)
        );
        assert!(
            is_tappable(&device),
            "{name:?} exposes its own input, so cpal would open that input side instead of \
             building a loopback tap — this is the wrong-source failure the app refuses. \
             Switch the default output device (or your input away from it) and re-run."
        );

        let config = device
            .default_output_config()
            .unwrap_or_else(|e| panic!("no output config for {name:?}: {e}"));
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        println!(
            "system-audio probe: {} Hz, {} channels, {sample_format:?}",
            stream_config.sample_rate, stream_config.channels
        );

        // Plain atomic bits rather than a Mutex: this is written from the
        // real-time audio callback, where locking is exactly what the rest of
        // this module is careful never to do.
        let peak_bits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let observe = {
            let peak_bits = peak_bits.clone();
            move |max: f32| {
                let bits = max.to_bits();
                let _ = peak_bits.fetch_max(bits, Ordering::Relaxed);
            }
        };

        let err_fn = |e: cpal::Error| panic!("system audio stream error: {e}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    observe(data.iter().fold(0.0_f32, |m, s| m.max(s.abs())));
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    observe(
                        data.iter()
                            .fold(0.0_f32, |m, s| m.max((*s as f32 / 32768.0).abs())),
                    );
                },
                err_fn,
                None,
            ),
            other => panic!("unsupported system audio sample format: {other:?}"),
        };

        let stream = stream
            .unwrap_or_else(|e| panic!("failed to build system audio stream on {name:?}: {e}"));
        stream
            .play()
            .unwrap_or_else(|e| panic!("failed to start system audio stream: {e}"));
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        drop(stream);

        let peak = f32::from_bits(peak_bits.load(Ordering::Relaxed));
        println!("system-audio probe: peak amplitude over {seconds}s = {peak:.6}");
        peak
    }
}
