use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::aec::AecHub;
use super::channel::{Channel, ChannelPipeline, StreamRoute};
use super::device_util::{device_id, device_name};
use super::system::CaptureInfo;

/// An input device that could be used as a microphone.
///
/// Mirrors `system::OutputDevice` but has no `tappable` field: tappability is
/// a system-audio-only concept (whether cpal builds a loopback tap versus
/// opening a plain input), and every input device is, definitionally, a
/// plain input already.
#[derive(Debug, Clone, Serialize)]
pub struct InputDevice {
    /// Stable identifier — this is what gets saved and passed back to
    /// `start_mic_capture`, never the display name.
    pub id: Option<String>,
    pub name: String,
    pub is_default: bool,
}

/// Lists input devices that could serve as the microphone.
pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
    let host = cpal::default_host();
    // Compared by id, not name: two devices can share a name, in which case a
    // name comparison would mark both as the default.
    let default_id = host.default_input_device().and_then(|d| device_id(&d));
    let devices = host
        .input_devices()
        .map_err(|e| format!("could not enumerate input devices: {e}"))?;

    Ok(devices
        .map(|d| {
            let id = device_id(&d);
            InputDevice {
                is_default: id.is_some() && id == default_id,
                name: device_name(&d),
                id,
            }
        })
        .collect())
}

/// Picks the input device to capture from, by stable id, or the system default.
///
/// Mirrors `system::resolve_device` exactly: a saved device id can outlive
/// the device (unplugged, or carried over from another machine), so this
/// falls back to the OS default rather than failing hard.
fn resolve_device(preferred: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    if let Some(want) = preferred {
        let found = host
            .input_devices()
            .map_err(|e| format!("could not enumerate input devices: {e}"))?
            .find(|d| device_id(d).as_deref() == Some(want));
        if let Some(d) = found {
            return Ok(d);
        }
        eprintln!("mic: saved input device {want:?} not found, using the default");
    }

    host.default_input_device()
        .ok_or_else(|| "no input device found".to_string())
}

/// The device's default config is usually best, but on some hardware
/// (certain Bluetooth headsets in call mode, some USB interfaces) it reports
/// a sample format we don't handle. Falls back to whatever supported format
/// the device actually offers, preferring `f32` (no conversion needed) then
/// `i16` (universal, cheap to convert). Without this, those devices would
/// hard-fail with "unsupported format" instead of just working.
///
/// Note this fallback is effectively Windows/ALSA-only. On macOS,
/// `default_input_config()` itself returns `Err` for anything that is not
/// linear-PCM f32/i16, and that error is propagated below before the fallback
/// is reached — so on the only platform this has ever run on, such a device
/// reports "no default input config" rather than being renegotiated.
fn pick_supported_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let default_cfg = device
        .default_input_config()
        .map_err(|e| format!("no default input config: {e}"))?;

    if matches!(
        default_cfg.sample_format(),
        cpal::SampleFormat::F32 | cpal::SampleFormat::I16
    ) {
        return Ok(default_cfg);
    }

    let configs: Vec<_> = device
        .supported_input_configs()
        .map_err(|e| format!("supported_input_configs failed: {e}"))?
        .collect();

    let candidates: Vec<(cpal::SampleFormat, u32, u32)> = configs
        .iter()
        .map(|r| (r.sample_format(), r.min_sample_rate(), r.max_sample_rate()))
        .collect();
    if let Some((index, rate)) = choose_config(&candidates) {
        return Ok(configs[index].clone().with_sample_rate(rate));
    }

    Err(format!(
        "mic exposes no supported format (need f32 or i16); default was {:?}",
        default_cfg.sample_format()
    ))
}

/// The rate every downstream default is tuned around. Not a hard requirement —
/// `vad.rs` is rate-parameterized — just the target to land nearest to.
const TARGET_RATE: u32 = 48_000;

/// Chooses which supported config to open, as `(index, sample_rate)`.
///
/// Split out as a pure function over plain tuples so it can be tested without
/// a device: the bug it exists to prevent was invisible on this machine and
/// only showed up when someone enumerated a real device's ranges by hand.
///
/// **The bug:** every backend that reports *discrete* rates emits one range
/// per rate, each with `min == max`. Picking the first range of the preferred
/// format therefore picks the first rate the backend happened to enumerate —
/// which is the lowest. On this machine's built-in microphone that silently
/// chose 44100 over the available 48000; on Windows, cpal iterates a
/// `COMMON_SAMPLE_RATES` table ascending from 5512 Hz, so the first `f32`
/// range is 5512 or 8000 Hz and the microphone would have opened there.
///
/// The upgrade to cpal 0.18 made this materially more reachable: `I32`/`I24`
/// now outrank `I16` in `default_input_config()`, so hardware that previously
/// returned `I16` directly at its mix rate now falls into this fallback.
fn choose_config(candidates: &[(cpal::SampleFormat, u32, u32)]) -> Option<(usize, u32)> {
    for preferred in [cpal::SampleFormat::F32, cpal::SampleFormat::I16] {
        let best = candidates
            .iter()
            .enumerate()
            .filter(|(_, (fmt, _, _))| *fmt == preferred)
            // Each range can deliver TARGET_RATE if it spans it, otherwise the
            // nearest endpoint. Pick whichever range lands closest to target,
            // breaking ties toward the higher rate.
            .map(|(i, (_, min, max))| (i, TARGET_RATE.clamp(*min, *max)))
            .min_by_key(|(_, rate)| (rate.abs_diff(TARGET_RATE), u32::MAX - rate));
        if best.is_some() {
            return best;
        }
    }
    None
}

/// A running mic capture. Dropping or `stop`ping it tears down the stream.
///
/// The `Drop` impl is the load-bearing half: this used to rely solely on an
/// explicit `stop()`, so any path that dropped the handle without calling it —
/// a panic, an early return, a poisoned `Mutex` in `AudioState` — left the
/// capture thread spinning forever with the stream open and the OS microphone
/// indicator lit, with no way left to reach it.
pub struct MicCaptureHandle {
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// What actually opened — sample rate, channel count, live device.
    /// Returned from `start_mic_capture` the same way system-audio already
    /// returns [`CaptureInfo`], so the Audio Inspector can show both ears
    /// without guessing.
    pub info: CaptureInfo,
}

impl MicCaptureHandle {
    /// Stops capture and waits for the thread to wind down. Equivalent to
    /// dropping the handle; kept as a named method because an explicit
    /// `handle.stop()` reads better at the call sites that have one.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for MicCaptureHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Diagnostic, not a fix: independently re-traced this teardown chain
        // (this crate's `start()` loop, `cpal` 0.18.2's `Stream`/`StreamInner`
        // — the disconnect monitor only holds a `Weak`, never a second strong
        // `Arc` that could outlive this — and `coreaudio-rs` 0.14.2's
        // `Drop for AudioUnit`, which calls `stop()` +
        // `AudioUnitUninitialize` + `AudioComponentInstanceDispose`) and
        // found no code defect. `t.join()` above only proves the capture
        // thread's stack frame ended, which is where `stream` and the OS
        // resources it owns actually drop — it does not, by itself, prove
        // *macOS* released the hardware or updated its mic-in-use indicator
        // on the same timescale, which is outside this process's control.
        // This line timestamps the one fact Aside itself can verify — log
        // this against a stopwatch on the OS orange-dot indicator to settle
        // whether any remaining delay is here or in the OS.
        eprintln!(
            "[{}ms since epoch] mic capture: thread joined, stream/AudioUnit torn down",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
    }
}

/// Starts capturing the microphone on a dedicated OS thread, and emits a
/// `mic-level` event once per audio callback with that buffer's RMS
/// (commonly 50–200×/second, depending on the device's buffer size). Blocks
/// the caller until the stream is confirmed running (or failed), so callers
/// get an honest success/error instead of finding out later that no audio
/// ever arrived.
///
/// `preferred_device`, when `Some`, pins capture to that stable device id
/// (see [`InputDevice::id`]) via [`resolve_device`]; `None` follows the OS
/// default, exactly as before this parameter existed.
pub fn start(
    app: AppHandle,
    preferred_device: Option<String>,
    stt_route: StreamRoute,
    aec: AecHub,
) -> Result<MicCaptureHandle, String> {
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
        let config = match pick_supported_config(&device) {
            Ok(c) => c,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };

        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let sample_rate = stream_config.sample_rate;
        // cpal delivers interleaved samples, so the channel count is required
        // to convert a buffer length into real elapsed time. Without it every
        // VAD timer runs at N× speed on a multi-channel device — and stereo
        // input is routine on USB interfaces, aggregate devices and webcams.
        let channels = stream_config.channels;
        let info = CaptureInfo {
            sample_rate,
            channels,
            device_name: device_name(&device),
            device_id: device_id(&device),
        };
        // Its own detector instance, never shared with the system channel:
        // the noise floor adapts to *this* source, and your room has nothing
        // to do with the far end of the call.
        let mut pipeline = ChannelPipeline::new(Channel::Mic);
        pipeline.set_stt_filter(Arc::new(aec));
        // Snapshotted now, announced only after `stream.play()` succeeds — the
        // pipeline is about to move into the audio callback, and announcing a
        // detector state for a stream that then fails to start would draw the
        // panel's threshold marker for a channel that never ran.
        let initial_status = pipeline.status();

        let status_app = app.clone();
        let err_app = app.clone();
        let err_fn = move |e: cpal::Error| {
            eprintln!("mic capture stream error: {e}");
            let _ = err_app.emit(Channel::Mic.error_event(), e.to_string());
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // `try_lock`, not `lock`: this runs on the real-time
                    // audio thread, which must never block. Contention is
                    // rare (only start/stop-transcription commands briefly
                    // hold this) and skipping one buffer's worth of
                    // STT-forwarding on that rare contention is harmless —
                    // the level meter and VAD above are entirely unaffected.
                    let stt_tx = stt_route.try_lock().ok().and_then(|g| g.clone());
                    pipeline.process_f32(&app, data, channels, sample_rate, stt_tx.as_ref());
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // NOTE: this path has never run against real i16 hardware
                    // — every machine tested so far reports f32. The
                    // normalization is unit-tested in `pcm.rs`
                    // (`i16_and_f32_devices_produce_the_same_level_reading`),
                    // but treat live i16 behavior as unverified.
                    let stt_tx = stt_route.try_lock().ok().and_then(|g| g.clone());
                    pipeline.process_i16(&app, data, channels, sample_rate, stt_tx.as_ref());
                },
                err_fn,
                None,
            ),
            other => {
                let _ = ready_tx.send(Err(format!(
                    "unsupported mic sample format: {other:?} (only f32/i16 are handled today)"
                )));
                return;
            }
        };

        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("failed to build input stream: {e}")));
                return;
            }
        };

        if let Err(e) = stream.play() {
            let _ = ready_tx.send(Err(format!("failed to start input stream: {e}")));
            return;
        }

        // Gives the panel a threshold and noise-floor value to draw from
        // immediately, rather than leaving the meter unmarked until the first
        // transition — which through a long quiet stretch could be never.
        let _ = status_app.emit(Channel::Mic.vad_state_event(), initial_status);

        let _ = ready_tx.send(Ok(info));

        while !thread_stop.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // `stream` drops here, which stops capture.
    });

    match ready_rx.recv() {
        Ok(Ok(info)) => Ok(MicCaptureHandle {
            stop_flag,
            thread: Some(thread),
            info,
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err("capture thread exited before it could start".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn dummy_capture_info() -> CaptureInfo {
        CaptureInfo {
            sample_rate: 48_000,
            channels: 1,
            device_name: "test-mic".into(),
            device_id: None,
        }
    }

    /// `MicCaptureHandle`'s `Drop` is "the load-bearing half" (see the
    /// type's doc comment) of turning the OS microphone off — this proves
    /// that contract at the state-machine level without a real audio
    /// device: a stand-in "capture thread" that only exits once the stop
    /// flag flips, exactly like `start()`'s real
    /// `while !thread_stop.load(...) { sleep }` loop, with `stream`
    /// dropping right after. A regression that made `stop()`/`Drop` return
    /// without actually joining the thread — leaving that stand-in
    /// `stream` drop still pending — would fail this.
    #[test]
    fn stop_flips_the_flag_and_blocks_until_the_capture_thread_actually_exits() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_flag.clone();
        let (exited_tx, exited_rx) = mpsc::channel::<()>();

        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            // Stands in for the real thread's "`stream` drops here" comment.
            let _ = exited_tx.send(());
        });

        let handle = MicCaptureHandle {
            stop_flag: stop_flag.clone(),
            thread: Some(thread),
            info: dummy_capture_info(),
        };

        handle.stop();

        assert!(
            stop_flag.load(Ordering::SeqCst),
            "stop() must flip the stop flag so the capture loop can notice"
        );
        assert!(
            exited_rx.try_recv().is_ok(),
            "stop() returned before the capture thread actually exited — the \
             OS resource it holds could still be alive even though stop() \
             has already returned"
        );
    }

    /// The scenario the type's own doc comment calls out by name: a handle
    /// dropped without an explicit `.stop()` call (a panic, an early
    /// return, a poisoned `AudioState` mutex) must tear down exactly the
    /// same way, not leak the thread.
    #[test]
    fn dropping_the_handle_without_calling_stop_still_tears_down_the_thread() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_flag.clone();
        let (exited_tx, exited_rx) = mpsc::channel::<()>();

        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = exited_tx.send(());
        });

        {
            let _handle = MicCaptureHandle {
                stop_flag: stop_flag.clone(),
                thread: Some(thread),
                info: dummy_capture_info(),
            };
            // `_handle` drops at the end of this block — no explicit
            // `.stop()` call, matching the panic/early-return case.
        }

        assert!(stop_flag.load(Ordering::SeqCst));
        assert!(
            exited_rx.try_recv().is_ok(),
            "the plain Drop path must join the thread just like stop() does"
        );
    }

    #[test]
    fn the_handle_carries_capture_info_the_inspector_can_read() {
        let info = dummy_capture_info();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_flag.clone();
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let handle = MicCaptureHandle {
            stop_flag,
            thread: Some(thread),
            info: info.clone(),
        };
        assert_eq!(handle.info.sample_rate, 48_000);
        assert_eq!(handle.info.channels, 1);
        assert_eq!(handle.info.device_name, "test-mic");
        handle.stop();
    }

    // ---- device resolution / listing --------------------------------------
    //
    // Unlike `system.rs`'s `same_physical_device`/`strip_device_role_suffix`
    // (plain string logic pulled out on purpose so it is testable without
    // hardware), `resolve_device` and `list_input_devices` here are thin
    // wrappers directly over `cpal::Host` — there is no pure sub-piece to
    // isolate without inventing a device-lookup trait purely to mock it,
    // which would be more machinery than this small feature warrants. So
    // both are exercised only by the one `#[ignore]`d hardware test below,
    // matching this project's honesty rule for hardware-dependent paths
    // rather than claiming unit coverage that doesn't exist.

    /// Diagnostic-plus-one-real-assertion: lists the real input devices this
    /// machine reports, resolves the default, and — the one thing worth
    /// actually checking — confirms a saved device id that no longer exists
    /// falls back to the default rather than failing capture outright (the
    /// same contract `system::resolve_device` already has, and the reason a
    /// device that was unplugged or carried over from another machine must
    /// not turn into a hard error).
    ///
    /// Run: `cargo test --lib audio::mic::tests::list_and_resolve_real_input_devices -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn list_and_resolve_real_input_devices() {
        let devices = list_input_devices().expect("input devices should enumerate on this machine");
        for d in &devices {
            println!(
                "in: name={:?} id={:?} default={}",
                d.name, d.id, d.is_default
            );
        }

        let resolved = resolve_device(None).expect("a default input device should resolve");
        println!("resolved default: {:?}", device_name(&resolved));

        let fallback = resolve_device(Some("definitely-not-a-real-device-id"))
            .expect("an unknown preferred id should fall back to the default, not error");
        println!("fallback resolved: {:?}", device_name(&fallback));
    }
}
