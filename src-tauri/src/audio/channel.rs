//! One audio channel's processing pipeline, shared by every capture source.
//!
//! Aside listens on two channels: `Mic` (you) and `System` (everyone else on
//! the call). They need identical treatment — downmix, level, voice-activity
//! detection — but must stay separately identified all the way to the UI,
//! because from Phase 5 onward the channel *is* the speaker label. There is no
//! diarization model anywhere in this design; keeping the two streams apart at
//! the source is what replaces it.
//!
//! Why a shared struct rather than a second copy of the mic loop: `mic.rs`
//! already carries the note that Natively ended up with two near-identical
//! audio loops which drifted apart, and this project inherited that lesson
//! before it had a second channel to get wrong. Now that it has one, the
//! anti-drift measure is structural — both callers run this same code, so a
//! fix to one is a fix to both by construction.
//!
//! Each channel owns its **own** `Vad`. That is deliberate and not shareable:
//! the detector adapts to a noise floor, and your room and the far end of the
//! call have unrelated noise. One shared detector would let a noisy room
//! desensitize detection of the other person.
//!
//! ## Why measurement is split from emitting
//!
//! [`ChannelPipeline::measure`] does all the real work and touches no
//! `AppHandle`; `process_f32`/`process_i16` are thin wrappers that measure and
//! then emit. That seam exists purely so the measurement is testable without a
//! running Tauri app — and it was added in response to a concrete failure. An
//! adversarial review reintroduced this codebase's historical "meter reads 41%
//! high" bug (level measured on the raw interleaved buffer while the detector
//! measured the downmix) and the entire 82-test suite still passed, because
//! every test covered the `pcm.rs` helpers in isolation and nothing covered the
//! code that calls them. Concentrating both channels into one pipeline had
//! concentrated the risk into the one file with no tests.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use super::pcm::{downmix_to_mono, normalize_i16_into, rms_level};
use super::vad::{Vad, VadConfig, VadEvent, VadState};

/// Which of the two audio sources a reading came from.
///
/// `Hash` added for Phase 7's `rag::indexer::RagIndexState`, which keys a
/// per-channel streaming chunker map by `(Uuid, Channel)` — purely additive,
/// no behavior change to anything that already uses this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// The local microphone — you.
    Mic,
    /// System output loopback — whoever else is on the call.
    System,
}

impl Channel {
    /// Event names are derived, never written out by hand at call sites.
    ///
    /// The frontend distinguishes the two channels purely by these strings, so
    /// a typo would be a silently dead listener rather than a build error.
    /// Deriving them from the enum means adding a channel cannot forget one —
    /// and the tests below pin the literal strings, because uniqueness alone
    /// would happily accept a consistent typo on both sides of the enum.
    pub fn level_event(self) -> &'static str {
        match self {
            Channel::Mic => "mic-level",
            Channel::System => "system-level",
        }
    }

    pub fn error_event(self) -> &'static str {
        match self {
            Channel::Mic => "mic-error",
            Channel::System => "system-error",
        }
    }

    /// Advisory messages that are *not* failures — currently only the system
    /// channel's "no audio heard yet" hint. Kept off `error_event` so the panel
    /// can render it as guidance rather than a red error the user can't clear.
    pub fn notice_event(self) -> &'static str {
        match self {
            Channel::Mic => "mic-notice",
            Channel::System => "system-notice",
        }
    }

    pub fn vad_state_event(self) -> &'static str {
        match self {
            Channel::Mic => "mic-vad-state",
            Channel::System => "system-vad-state",
        }
    }

    pub fn vad_event(self) -> &'static str {
        match self {
            Channel::Mic => "mic-vad-event",
            Channel::System => "system-vad-event",
        }
    }
}

/// What one callback's audio produces for a speech-to-text consumer, as
/// opposed to the UI-facing events `emit` sends over Tauri's IPC bridge.
///
/// Defined here rather than in `stt`/`pipeline` so `audio` stays the single
/// owner of "what a channel callback can produce" — `pipeline.rs` is what
/// consumes this to drive an `SttEngine`, but `audio` has no reciprocal
/// knowledge of STT at all, only of this plain data shape.
#[derive(Debug, Clone)]
pub enum StreamMsg {
    /// Mono PCM for the active speech-to-text engine.
    Audio { samples: Vec<f32>, sample_rate: u32 },
    /// UI VAD saw the speaker stop. Does **not** cut audio to STT.
    /// Pipeline may ask Speechmatics for ForceEndOfUtterance at most once
    /// per turn. Parakeet already finals on `[EOU]`.
    SpeechEnd,
}

/// Shared cell a transcription task's sender lives in, checked once per
/// audio callback (`mic.rs`/`system.rs`). `None` means "no transcription
/// active" — capture, the level meter, and VAD all run identically either
/// way; this only gates the extra fan-out to an `SttEngine`. See
/// `pipeline.rs`.
pub type StreamRoute = std::sync::Arc<std::sync::Mutex<Option<std::sync::mpsc::Sender<StreamMsg>>>>;

/// Optional hook on the STT fan-out. The cpal callback must not run DSP here —
/// implementations may only `try_send` onto another thread.
pub trait SttAudioFilter: Send + Sync {
    /// Mic path: return true if this buffer will be forwarded (possibly cleaned)
    /// by the filter, so the pipeline should not send the raw copy.
    fn intercept_capture(
        &self,
        samples: &[f32],
        sample_rate: u32,
        tx: &std::sync::mpsc::Sender<StreamMsg>,
    ) -> bool;
    /// System path: observe a copy of what played. Others STT still gets the
    /// original samples from the pipeline.
    fn tap_render(&self, samples: &[f32], sample_rate: u32);
}

/// A loudness reading for one channel.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ChannelLevel {
    pub channel: Channel,
    pub rms: f32,
}

/// What the panel needs to draw a speaking indicator: the current state plus
/// where the "speech starts here" line sits on the level meter.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct VadStatus {
    pub channel: Channel,
    pub state: VadState,
    pub speaking: bool,
    pub enter_threshold: f32,
    pub noise_floor: f32,
}

/// Everything one callback's audio produced, before any of it is emitted.
#[derive(Debug, Default)]
pub struct Measurement {
    pub rms: f32,
    pub events: Vec<VadEvent>,
    pub transitions: Vec<VadState>,
}

/// Everything one capture stream needs to turn raw device buffers into
/// frontend events. Lives on the audio callback's thread and is reused across
/// callbacks so the real-time path never allocates.
pub struct ChannelPipeline {
    channel: Channel,
    vad: Vad,
    /// The downmixed mono signal both the meter and the detector measure.
    /// Always populated, including for already-mono input: the extra memcpy is
    /// a couple of microseconds and it means there is exactly one buffer any
    /// observer can read, rather than a second downmix elsewhere.
    mono: Vec<f32>,
    /// Normalized-but-still-interleaved `i16` input on its way to `mono`.
    i16_scratch: Vec<f32>,
    stt_filter: Option<Arc<dyn SttAudioFilter>>,
}

impl ChannelPipeline {
    pub fn new(channel: Channel) -> Self {
        Self {
            channel,
            vad: Vad::new(VadConfig::default()),
            mono: Vec::new(),
            i16_scratch: Vec::new(),
            stt_filter: None,
        }
    }

    pub fn set_stt_filter(&mut self, filter: Arc<dyn SttAudioFilter>) {
        self.stt_filter = Some(filter);
    }

    /// The detector's current state, as the frontend sees it.
    ///
    /// Snapshotted before the pipeline moves into the audio callback so the
    /// caller can announce it *after* the stream is confirmed running. Emitting
    /// it earlier would draw the panel's threshold marker for a channel whose
    /// stream might still fail to build or start.
    pub fn status(&self) -> VadStatus {
        let state = self.vad.state();
        VadStatus {
            channel: self.channel,
            state,
            speaking: state.is_speaking(),
            enter_threshold: self.vad.enter_threshold(),
            noise_floor: self.vad.noise_floor(),
        }
    }

    /// Downmixes `f32` input into `self.mono` and measures it. Emits nothing.
    pub fn measure_f32(&mut self, data: &[f32], channels: u16, sample_rate: u32) -> Measurement {
        self.mono.clear();
        if channels <= 1 {
            self.mono.extend_from_slice(data);
        } else {
            downmix_to_mono(data, channels, &mut self.mono);
        }
        self.measure(sample_rate)
    }

    /// Normalizes and downmixes `i16` input into `self.mono` and measures it.
    ///
    /// Normalizing to the same ±1.0 range the `f32` path uses is what lets one
    /// set of VAD thresholds be correct for both formats.
    pub fn measure_i16(&mut self, data: &[i16], channels: u16, sample_rate: u32) -> Measurement {
        self.i16_scratch.clear();
        normalize_i16_into(data, &mut self.i16_scratch);
        self.mono.clear();
        downmix_to_mono(&self.i16_scratch, channels, &mut self.mono);
        self.measure(sample_rate)
    }

    /// The one place level and voice-activity are computed, both from the same
    /// `self.mono` buffer.
    ///
    /// That single shared source is the whole point: when the meter measured
    /// the raw interleaved buffer while the detector measured the downmix, a
    /// mono mic on a stereo interface made the meter read ~41% high against the
    /// very threshold line drawn beside it — the bar would visibly cross the
    /// line while the indicator stayed dark.
    fn measure(&mut self, sample_rate: u32) -> Measurement {
        // Disjoint field borrows: the detector needs `&mut`, the buffer `&`.
        let Self { vad, mono, .. } = self;
        Measurement {
            rms: rms_level(mono),
            // Already mono, so one channel — see `vad.rs` for why this never
            // decides whether the audio continues anywhere.
            events: vad.push_samples(mono, sample_rate, 1),
            transitions: vad.take_transitions(),
        }
    }

    /// Processes one `f32` callback buffer, returning the mono signal it
    /// derived so a caller can inspect it without downmixing a second time.
    ///
    /// `stt_tx`, when present, forwards the mono audio from this buffer to a
    /// transcription consumer — see `pipeline.rs`. VAD events stay on the
    /// UI path only (`emit`); every live engine has its own stop-of-speech.
    /// Kept as an `Option` rather than a second, STT-specific method: the
    /// level meter and VAD keep working identically whether or not
    /// transcription is active.
    pub fn process_f32(
        &mut self,
        app: &AppHandle,
        data: &[f32],
        channels: u16,
        sample_rate: u32,
        stt_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>,
    ) -> &[f32] {
        let m = self.measure_f32(data, channels, sample_rate);
        self.forward_to_stt(stt_tx, sample_rate);
        self.forward_speech_end(stt_tx, &m.events);
        self.emit(app, m);
        &self.mono
    }

    /// Processes one `i16` callback buffer. See `process_f32` for `stt_tx`.
    pub fn process_i16(
        &mut self,
        app: &AppHandle,
        data: &[i16],
        channels: u16,
        sample_rate: u32,
        stt_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>,
    ) -> &[f32] {
        let m = self.measure_i16(data, channels, sample_rate);
        self.forward_to_stt(stt_tx, sample_rate);
        self.forward_speech_end(stt_tx, &m.events);
        self.emit(app, m);
        &self.mono
    }

    /// A `Sender::send` on a real-time audio callback thread is only as safe
    /// as the channel it targets — this assumes `stt_tx` is unbounded (an
    /// `std::sync::mpsc::channel()`, never `sync_channel`), so `send` never
    /// blocks. `pipeline.rs`'s constructor is the single place that decides
    /// that, matching the same "never block the audio thread" discipline
    /// `audio/mic.rs`'s and `audio/system.rs`'s own doc comments already
    /// state. A closed receiver (the transcription task stopped) is not an
    /// error worth surfacing here — the caller already stopped listening.
    fn forward_to_stt(
        &self,
        stt_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>,
        sample_rate: u32,
    ) {
        let Some(tx) = stt_tx else {
            return;
        };
        match self.channel {
            Channel::Mic => {
                if let Some(filter) = &self.stt_filter {
                    if filter.intercept_capture(&self.mono, sample_rate, tx) {
                        return;
                    }
                }
            }
            Channel::System => {
                if let Some(filter) = &self.stt_filter {
                    filter.tap_render(&self.mono, sample_rate);
                }
            }
        }
        let _ = tx.send(StreamMsg::Audio {
            samples: self.mono.clone(),
            sample_rate,
        });
    }

    /// ForceEOU hint only. Audio still flows independently — VAD never gates STT.
    fn forward_speech_end(
        &self,
        stt_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>,
        events: &[VadEvent],
    ) {
        let Some(tx) = stt_tx else {
            return;
        };
        if events
            .iter()
            .any(|e| matches!(e, VadEvent::SpeechEnd { .. }))
        {
            let _ = tx.send(StreamMsg::SpeechEnd);
        }
    }

    fn emit(&self, app: &AppHandle, m: Measurement) {
        let _ = app.emit(
            self.channel.level_event(),
            ChannelLevel {
                channel: self.channel,
                rms: m.rms,
            },
        );
        for event in m.events {
            let _ = app.emit(self.channel.vad_event(), event);
        }
        // Announce every transition, not just the net state. One oversized
        // buffer can contain a whole speech cycle (Idle -> Speech -> Hangover
        // -> Idle); reporting only where it ended up would say "idle" and
        // leave the speaking indicator dark through an entire utterance.
        for state in m.transitions {
            self.emit_status(app, state);
        }
    }

    fn emit_status(&self, app: &AppHandle, state: VadState) {
        // Thresholds move on a multi-second time constant, so reading the
        // current value for a transition that happened a few frames ago is
        // accurate to well under a pixel of meter travel.
        let status = VadStatus {
            state,
            speaking: state.is_speaking(),
            ..self.status()
        };
        let _ = app.emit(self.channel.vad_state_event(), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interleaved stereo where only the left channel carries audio — a mono
    /// mic on a stereo interface, the most common stereo-input case.
    fn half_dead_stereo(frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames * 2)
            .map(|i| if i % 2 == 0 { amplitude } else { 0.0 })
            .collect()
    }

    // ---- the pipeline actually measures what it claims to ----------------

    #[test]
    fn level_is_measured_on_the_downmix_not_the_raw_interleaved_buffer() {
        // THE regression test for this module. An adversarial review proved
        // that changing `measure` to read the raw buffer instead of the
        // downmix left all 82 tests passing, because every existing test
        // covered `pcm.rs`'s helpers in isolation and none covered the code
        // that calls them. This one exercises the real pipeline.
        let mut p = ChannelPipeline::new(Channel::Mic);
        let interleaved = half_dead_stereo(512, 0.4);
        let m = p.measure_f32(&interleaved, 2, 48_000);

        // Averaging (0.4, 0.0) frames gives 0.2.
        assert!(
            (m.rms - 0.2).abs() < 1e-4,
            "expected the downmixed level 0.2, got {}",
            m.rms
        );
        // Guard: the wrong answer must be meaningfully different, or this
        // test would pass either way and prove nothing.
        let raw = rms_level(&interleaved);
        assert!(
            raw > m.rms * 1.3,
            "guard: raw reading ({raw}) must really be inflated vs {} — \
             this test is meaningless if they already agree",
            m.rms
        );
    }

    #[test]
    fn the_detector_sees_the_same_buffer_the_level_was_measured_from() {
        // The meter and the detector diverging is the failure this pipeline
        // exists to prevent, so pin that `mono()` is what was measured.
        let mut p = ChannelPipeline::new(Channel::Mic);
        let m = p.measure_f32(&half_dead_stereo(512, 0.4), 2, 48_000);
        assert!((rms_level(&p.mono) - m.rms).abs() < 1e-6);
        assert_eq!(
            p.mono.len(),
            512,
            "mono buffer should hold one value per frame"
        );
    }

    #[test]
    fn i16_and_f32_inputs_produce_the_same_level_through_the_real_pipeline() {
        // pcm.rs tests that the *helpers* agree. This tests that the pipeline
        // actually routes both formats through them the same way — the i16
        // path has never run on real hardware, so this is most of what stands
        // behind it.
        let f32_frames: Vec<f32> = half_dead_stereo(512, 0.5);
        let i16_frames: Vec<i16> = f32_frames
            .iter()
            .map(|&s| (s * 32768.0).round() as i16)
            .collect();

        let mut a = ChannelPipeline::new(Channel::Mic);
        let mut b = ChannelPipeline::new(Channel::Mic);
        let from_f32 = a.measure_f32(&f32_frames, 2, 48_000);
        let from_i16 = b.measure_i16(&i16_frames, 2, 48_000);

        assert!(
            (from_f32.rms - from_i16.rms).abs() < 1e-4,
            "f32 gave {} but i16 gave {} — one set of VAD thresholds cannot be \
             correct for both if these disagree",
            from_f32.rms,
            from_i16.rms
        );
    }

    #[test]
    fn a_mono_device_is_measured_at_full_level_not_halved() {
        // The `channels <= 1` branch takes a different path through
        // `measure_f32`. If it ever fell through to the downmix with a stale
        // channel count, a mono device's level would be silently wrong.
        let mut p = ChannelPipeline::new(Channel::Mic);
        let m = p.measure_f32(&[0.5; 480], 1, 48_000);
        assert!((m.rms - 0.5).abs() < 1e-6, "got {}", m.rms);
        assert_eq!(p.mono.len(), 480);
    }

    #[test]
    fn buffers_are_reused_across_callbacks_rather_than_regrown() {
        // The real-time audio thread must not allocate. Capacity surviving
        // across calls is what proves the buffers are reused.
        let mut p = ChannelPipeline::new(Channel::Mic);
        p.measure_f32(&half_dead_stereo(512, 0.3), 2, 48_000);
        let cap = p.mono.capacity();
        assert!(cap >= 512);
        for _ in 0..50 {
            p.measure_f32(&half_dead_stereo(512, 0.3), 2, 48_000);
        }
        assert_eq!(
            p.mono.capacity(),
            cap,
            "mono buffer regrew — it is not being reused"
        );
    }

    #[test]
    fn a_shorter_buffer_does_not_leave_stale_samples_behind() {
        // `mono` is cleared, not truncated-by-overwrite. A missing clear would
        // leave the tail of a previous, longer callback appended to this one.
        let mut p = ChannelPipeline::new(Channel::Mic);
        p.measure_f32(&half_dead_stereo(512, 0.9), 2, 48_000);
        p.measure_f32(&half_dead_stereo(64, 0.0), 2, 48_000);
        assert_eq!(p.mono.len(), 64);
        assert!(
            p.mono.iter().all(|&s| s == 0.0),
            "stale loud samples survived"
        );
    }

    // ---- channel identity -------------------------------------------------

    #[test]
    fn event_names_are_exactly_the_strings_the_frontend_listens_for() {
        // Uniqueness alone is not enough: a review proved that renaming
        // "mic-level" to "mic-lvl" kept the old uniqueness test green while
        // silently disconnecting the panel. These literals are duplicated in
        // `src/PanelWindow.tsx`'s `events()` helper — change both together.
        assert_eq!(Channel::Mic.level_event(), "mic-level");
        assert_eq!(Channel::Mic.error_event(), "mic-error");
        assert_eq!(Channel::Mic.notice_event(), "mic-notice");
        assert_eq!(Channel::Mic.vad_state_event(), "mic-vad-state");
        assert_eq!(Channel::Mic.vad_event(), "mic-vad-event");
        assert_eq!(Channel::System.level_event(), "system-level");
        assert_eq!(Channel::System.error_event(), "system-error");
        assert_eq!(Channel::System.notice_event(), "system-notice");
        assert_eq!(Channel::System.vad_state_event(), "system-vad-state");
        assert_eq!(Channel::System.vad_event(), "system-vad-event");
    }

    #[test]
    fn every_event_name_is_unique_across_channels_and_kinds() {
        let mut names = Vec::new();
        for ch in [Channel::Mic, Channel::System] {
            names.push(ch.level_event());
            names.push(ch.error_event());
            names.push(ch.notice_event());
            names.push(ch.vad_state_event());
            names.push(ch.vad_event());
        }
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "duplicate event name among {names:?}"
        );
    }

    #[test]
    fn channel_names_serialize_as_the_lowercase_strings_the_frontend_expects() {
        // The panel discriminates on this value inside the event payload.
        assert_eq!(serde_json::to_string(&Channel::Mic).unwrap(), "\"mic\"");
        assert_eq!(
            serde_json::to_string(&Channel::System).unwrap(),
            "\"system\""
        );
    }

    #[test]
    fn stt_fanout_is_audio_only_even_when_vad_fires() {
        // Speaking-light VAD still runs (events exist on the measurement),
        // but those events must not travel to STT. A regression that sent
        // StreamMsg::Vad again would over-segment cloud/EOU engines.
        let mut p = ChannelPipeline::new(Channel::Mic);
        let (tx, rx) = std::sync::mpsc::channel();
        let loud = vec![0.5_f32; 480];
        for _ in 0..80 {
            let _m = p.measure_f32(&loud, 1, 48_000);
            p.forward_to_stt(Some(&tx), 48_000);
        }
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(!msgs.is_empty(), "audio must still be forwarded to STT");
        assert!(
            msgs.iter().all(|m| matches!(m, StreamMsg::Audio { .. })),
            "VAD must stay off the STT path, got {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, StreamMsg::Audio { samples, .. } if !samples.is_empty())),
            "forwarded audio must carry samples"
        );
    }

    struct DropCapture;
    impl SttAudioFilter for DropCapture {
        fn intercept_capture(
            &self,
            _samples: &[f32],
            _sample_rate: u32,
            _tx: &std::sync::mpsc::Sender<StreamMsg>,
        ) -> bool {
            true
        }
        fn tap_render(&self, _samples: &[f32], _sample_rate: u32) {}
    }

    #[test]
    fn mic_stt_filter_can_take_the_buffer_instead_of_the_raw_copy() {
        let mut p = ChannelPipeline::new(Channel::Mic);
        p.set_stt_filter(std::sync::Arc::new(DropCapture));
        let (tx, rx) = std::sync::mpsc::channel();
        p.forward_to_stt(Some(&tx), 16_000);
        assert!(
            rx.try_recv().is_err(),
            "when AEC intercepts, the raw mic copy must not also go to Me STT"
        );
    }

    struct RecordRender(std::sync::Mutex<usize>);
    impl SttAudioFilter for RecordRender {
        fn intercept_capture(
            &self,
            _samples: &[f32],
            _sample_rate: u32,
            _tx: &std::sync::mpsc::Sender<StreamMsg>,
        ) -> bool {
            false
        }
        fn tap_render(&self, samples: &[f32], _sample_rate: u32) {
            *self.0.lock().unwrap() += samples.len();
        }
    }

    #[test]
    fn system_stt_filter_sees_a_copy_but_others_still_get_the_original() {
        let mut p = ChannelPipeline::new(Channel::System);
        let tap = std::sync::Arc::new(RecordRender(std::sync::Mutex::new(0)));
        p.set_stt_filter(tap.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        p.measure_f32(&[0.25, 0.5, 0.75], 1, 16_000);
        p.forward_to_stt(Some(&tx), 16_000);
        assert_eq!(*tap.0.lock().unwrap(), 3);
        match rx.try_recv().expect("Others STT must still get audio") {
            StreamMsg::Audio { samples, .. } => assert_eq!(samples, vec![0.25, 0.5, 0.75]),
            other => panic!("expected Audio, got {other:?}"),
        }
    }

    #[test]
    fn one_channels_noise_floor_does_not_move_the_others() {
        // Two pipelines must adapt independently. The previous version of this
        // test compared the addresses of two stack locals, which can never be
        // equal — it passed even when both pipelines were the same channel.
        // This drives them with genuinely different audio instead, so it would
        // fail if `Vad` ever became shared state.
        let mut loud = ChannelPipeline::new(Channel::Mic);
        let mut quiet = ChannelPipeline::new(Channel::System);

        // A noisy room on one channel, near-silence on the other.
        for _ in 0..200 {
            loud.measure_f32(&[0.05; 480], 1, 48_000);
            quiet.measure_f32(&[0.0005; 480], 1, 48_000);
        }

        let loud_floor = loud.vad.noise_floor();
        let quiet_floor = quiet.vad.noise_floor();
        assert!(
            loud_floor > quiet_floor * 2.0,
            "the noisy channel's floor ({loud_floor}) should have adapted well above \
             the quiet one's ({quiet_floor}); if these track together the detector \
             state is being shared"
        );
    }
}
