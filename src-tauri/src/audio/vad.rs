//! Voice activity detection — **segmentation and UI signal only.**
//!
//! # This module must never gate audio
//!
//! Nothing here returns, buffers, or withholds audio samples. VAD tells you
//! *when* someone started and stopped talking; it never decides *whether*
//! audio reaches speech-to-text. That rule is enforced by the type signatures
//! rather than by comments: [`Vad::push_samples`] takes `&[f32]` and returns
//! `Vec<VadEvent>`, so there is physically no audio-shaped value for a caller
//! to accidentally treat as "the audio that survived the gate".
//!
//! This is deliberate, and both reference implementations are why:
//!
//! - `references/Repo2-OpenCluely` gates its speech-to-text on VAD. Audio is
//!   only appended to the transcription buffer while the state machine says
//!   "speaking", and there are three separate paths where audio is destroyed
//!   before transcription ever sees it. When their noise floor drifts up or
//!   the mic is quiet, *zero* audio is delivered and the user gets silence
//!   with no error.
//! - `references/Repo1-Natively` has a `vad.rs` whose own header says
//!   "UI ONLY / It does NOT gate or delay audio sent to STT", and which keeps
//!   the old gating call reachable behind a type alias labelled
//!   "WARNING: This is the OLD pattern that causes latency issues". That file
//!   is now dead code — never registered as a module — and the gating logic
//!   it warned about migrated into the live audio path anyway. A comment did
//!   not hold that line. A signature does.
//!
//! Their design principles file puts the reason bluntly: hangover is a
//! bandwidth/cost knob, *not* a first-word-accuracy knob. If lengthening a
//! VAD timer looks like the fix for clipped words, the mechanism is wrong.
//!
//! # Why there is no pre-roll audio buffer
//!
//! Both references keep a few hundred milliseconds of audio buffered before
//! speech onset so the first syllable survives the gate. Aside has no gate, so
//! that audio is already downstream — buffering a second copy would cost
//! memory and copies to solve a problem this architecture doesn't have.
//!
//! Instead, [`VadEvent::SpeechStart`] reports a timestamp **backdated** by
//! `preroll_ms`. Downstream segmentation marks the utterance as having begun
//! slightly before detection, which is the actual goal: no clipped first word,
//! zero buffered audio.

use serde::Serialize;

use super::pcm::rms_level;

/// Where the speaker currently is, as far as this detector can tell.
///
/// `Hangover` is "recently talking, waiting to see if they're really done" —
/// it exists so a natural mid-sentence pause doesn't split one utterance into
/// two. Callers that just want a speaking indicator should use
/// [`VadState::is_speaking`], which treats `Hangover` as still speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VadState {
    Idle,
    Speech,
    Hangover,
}

impl VadState {
    /// True while the user should be shown as talking. `Hangover` counts:
    /// flickering the indicator off during a mid-sentence breath is worse
    /// than leaving it on a few hundred milliseconds too long.
    pub fn is_speaking(self) -> bool {
        matches!(self, VadState::Speech | VadState::Hangover)
    }
}

/// Something worth telling the rest of the app about. Note that no variant
/// carries audio — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VadEvent {
    /// Speech began. `at_ms` is backdated by `preroll_ms` so a segmenter
    /// that trusts it won't clip the first syllable.
    SpeechStart { at_ms: u64 },
    /// A complete utterance ended.
    ///
    /// All three fields are on the same timeline and mean different things,
    /// deliberately:
    /// - `started_at_ms` — the matching [`VadEvent::SpeechStart`]'s timestamp,
    ///   repeated so a consumer never has to correlate events by hand.
    /// - `at_ms` — when the speaker actually stopped, with the hangover wait
    ///   already subtracted. `at_ms - started_at_ms` is the real span.
    /// - `voiced_ms` — how much of that span was *actually* above threshold,
    ///   excluding internal pauses. Always `<= at_ms - started_at_ms`.
    ///
    /// `voiced_ms == 0` with a zero-length span is legitimate and means "the
    /// speaker stopped within the hangover following a [`VadEvent::ForcedBoundary`],
    /// so there is no new audio since that cut". A consumer should treat it as
    /// end-of-speech but should *not* create a segment for it. Suppressing the
    /// event entirely was tried and is wrong: the boundary already closed the
    /// previous segment, so with nothing emitted here a consumer is never told
    /// speech ended at all and waits forever. An empty segment is a cosmetic
    /// problem; a missing terminator is a correctness one.
    ///
    /// Named `voiced_ms` rather than `duration_ms` on purpose: an earlier
    /// revision called it `duration_ms`, which invited `at_ms - duration_ms`
    /// as a way to find the start. That silently produced a timestamp up to a
    /// second off, because it mixes a wall-clock instant with a voiced-only
    /// total.
    SpeechEnd {
        started_at_ms: u64,
        at_ms: u64,
        voiced_ms: u32,
    },
    /// A burst crossed the entry threshold but never accumulated enough
    /// voiced audio to be speech — a cough, a key click, a desk bump.
    ///
    /// **This discards a boundary marker, not audio.** OpenCluely's
    /// equivalent path empties the transcription buffer, which (per their own
    /// code) can also destroy a *previous* utterance that hadn't finished
    /// transcribing. Here there is nothing to destroy.
    NoiseDiscarded { voiced_ms: u32 },
    /// The speaker passed `max_utterance_ms` without pausing, so the
    /// utterance is cut here to bound downstream work.
    ///
    /// The state machine deliberately **stays in `Speech`** across this cut.
    /// OpenCluely resets to idle here, which means the very next frame has to
    /// clear the higher *entry* threshold mid-word to resume — a documented
    /// way to drop a syllable exactly at the seam.
    ForcedBoundary { at_ms: u64 },
}

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// Analysis frame size. Longer frames average a sharp consonant together
    /// with the silence around it and can push it under the threshold;
    /// OpenCluely evaluates on ~256 ms chunks and suffers exactly that.
    /// Natively uses 20 ms and documents the tradeoff as latency vs overhead.
    pub frame_ms: u32,
    /// Time constant for the background-noise estimate.
    ///
    /// Deliberately a *time* constant rather than a raw per-frame smoothing
    /// factor: both references hardcode a per-chunk factor, so their real
    /// adaptation rate silently changes whenever the audio buffer size does.
    /// Deriving the factor from `frame_ms / noise_floor_tau_ms` makes the
    /// behavior identical at any buffer size.
    pub noise_floor_tau_ms: f32,
    /// Speech starts above `noise_floor * enter_multiplier`.
    pub enter_multiplier: f32,
    /// Speech continues until below `noise_floor * exit_multiplier`. Lower
    /// than the entry multiplier on purpose — it should take more energy to
    /// *start* an utterance than to keep one going, so a dip mid-sentence
    /// doesn't end it.
    pub exit_multiplier: f32,
    /// Hard lower bound on the entry threshold, as normalized RMS. Stops a
    /// pathologically quiet room from driving the threshold to ~0 and making
    /// every tiny noise register as speech.
    pub absolute_floor: f32,
    /// How long the level must stay low before speech is considered over.
    ///
    /// Natively used 500 ms, raised from 150 ms with the comment "prevent
    /// clipping trailing consonants (s, t, etc)". Raised further to 700 ms
    /// here after live testing: an ordinary breath or comma-pause between
    /// clauses in conversational speech routinely runs 400-600 ms, and at
    /// 500 ms flat those normal pauses were closing out the utterance —
    /// splitting one sentence into two separate finalized lines mid-thought.
    /// Still well under a real turn-taking gap.
    pub hangover_ms: u32,
    /// Voiced audio required before a burst counts as a real utterance.
    ///
    /// Impulse noises (key clicks, desk bumps) are 10–50 ms. The shortest
    /// genuine words — "yes", "no", "ok" — run 150–300 ms. 200 ms sits above
    /// the impulses and below the words. Biased deliberately toward
    /// sensitivity: because this detector never gates audio, a false positive
    /// costs an indicator flicker, while a false negative could mislabel the
    /// start of a real answer.
    pub min_utterance_ms: u32,
    /// Cut a monologue after this long even without a pause.
    pub max_utterance_ms: u32,
    /// How far to backdate `SpeechStart`. See the module docs — this is a
    /// timestamp adjustment, not an audio buffer.
    pub preroll_ms: u32,
    /// Consecutive frames above the entry threshold required to declare
    /// speech. A single loud frame opening an utterance is how a door slam
    /// becomes a transcription attempt; OpenCluely has no such confirmation
    /// and relies entirely on discarding it after the fact.
    pub onset_frames: u32,
}

// KNOWN LIMITATION — steady noise above the entry threshold pins the detector.
//
// If the room's steady level sits above `enter_threshold` (a loud AC unit,
// road noise in a car, a fan spinning up mid-session), the detector enters
// `Speech`, every frame stays voiced, and because the noise floor only adapts
// while idle it can never climb out. The result is a speaking indicator stuck
// on and a `ForcedBoundary` every `max_utterance_ms` indefinitely.
//
// An escape hatch was implemented for this and then **reverted**, because it
// was a worse bug than the one it fixed. It armed on "N forced boundaries with
// no intervening pause", intending to catch machine noise (which never pauses)
// but not speech (which does). It does not separate them: `consecutive_forced`
// only resets on a *full* 500 ms hangover, so ordinary sub-500 ms breaths never
// reset it. Someone answering an open-ended interview question would arm it in
// about a minute, at which point the floor climbed onto their own voice and cut
// them off mid-sentence — and then, with their level now below the raised
// threshold, their continuing speech was reclassified as background and fed to
// the *fast* idle average, which chased their voice upward and left the
// detector completely deaf until they stopped talking for several seconds.
//
// That trades a cosmetic stuck-on indicator for silently losing a talking
// human, which is the wrong direction. Elapsed time cannot distinguish speech
// from machine noise; the discriminator has to be something like level
// *variance* over a window (speech has wide dynamic range, a fan has almost
// none). Left unfixed deliberately until that exists — the current failure is
// visible and harmless, the attempted fix was neither.

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            noise_floor_tau_ms: 2_000.0,
            enter_multiplier: 2.5,
            exit_multiplier: 1.6,
            absolute_floor: 0.008,
            hangover_ms: 700,
            min_utterance_ms: 200,
            max_utterance_ms: 15_000,
            preroll_ms: 300,
            onset_frames: 2,
        }
    }
}

impl VadConfig {
    /// Clamps nonsensical values into a working range.
    ///
    /// The fields are `pub` for tuning, which means callers can express
    /// combinations that break the state machine outright — `enter_multiplier`
    /// of 0 divides by zero in the exit threshold, `max_utterance_ms` of 0
    /// fires a forced boundary on *every frame* (50 IPC messages a second from
    /// the real-time audio thread), and `min > max` makes a normal
    /// `SpeechEnd` literally unreachable.
    ///
    /// Clamping rather than returning an error is deliberate: this is a
    /// personal app with one caller, and a detector that quietly works is
    /// better than a `Result` nobody handles.
    ///
    /// Deliberately *not* `debug_assert`-guarded. An earlier revision asserted
    /// each precondition here and it was straightforwardly self-contradictory:
    /// the whole contract is "bad values are clamped into something workable",
    /// and a debug panic makes the clamp unreachable in exactly the builds
    /// where it's being exercised — including its own tests.
    fn sanitized(mut self) -> Self {
        // NaN first, before any clamping. `f32::clamp` *propagates* NaN while
        // `f32::max` drops it, so a NaN field would survive some guards below
        // and not others. A NaN `exit_multiplier` is especially nasty: every
        // comparison against it is false, so continuous speech becomes an
        // endless start/discard cycle — roughly two spurious utterances per
        // second, each one an IPC message off the real-time audio thread.
        let d = Self::default();
        if !self.noise_floor_tau_ms.is_finite() {
            self.noise_floor_tau_ms = d.noise_floor_tau_ms;
        }
        if !self.enter_multiplier.is_finite() {
            self.enter_multiplier = d.enter_multiplier;
        }
        if !self.exit_multiplier.is_finite() {
            self.exit_multiplier = d.exit_multiplier;
        }
        if !self.absolute_floor.is_finite() {
            self.absolute_floor = d.absolute_floor;
        }

        // Order matters below: `frame_ms` is clamped first because
        // `hangover_ms`'s lower bound is derived from it, and `clamp` panics
        // if its low exceeds its high.
        self.frame_ms = self.frame_ms.clamp(1, 1_000);
        self.noise_floor_tau_ms = self.noise_floor_tau_ms.max(1.0);
        self.enter_multiplier = self.enter_multiplier.max(1.0);
        // Deliberately a proportional gap, not `enter_multiplier - EPSILON`.
        // `f32::EPSILON` is one ulp only for values in [1.0, 2.0); at the
        // default `enter_multiplier` of 2.5 it is *half* an ulp, so the
        // subtraction rounds straight back to 2.5 and the clamp silently
        // produced `exit == enter` — zero hysteresis width, i.e. the module's
        // central mechanism quietly disabled.
        self.exit_multiplier = self
            .exit_multiplier
            .clamp(0.01, self.enter_multiplier * 0.95);
        // Upper bound too: an absurdly high floor makes the entry threshold
        // unreachable and the detector permanently deaf. 1.0 is full scale.
        self.absolute_floor = self.absolute_floor.clamp(f32::MIN_POSITIVE, 1.0);
        self.max_utterance_ms = self.max_utterance_ms.max(self.frame_ms);
        self.min_utterance_ms = self.min_utterance_ms.min(self.max_utterance_ms);
        self.onset_frames = self.onset_frames.clamp(1, 1_000);
        // Bounded so an utterance always eventually ends and the indicator
        // can't stick on forever; and so backdating can't precede the session.
        self.hangover_ms = self.hangover_ms.clamp(self.frame_ms, 10_000);
        self.preroll_ms = self.preroll_ms.min(5_000);
        self
    }
}

/// The detector. Drive it with [`Vad::push_samples`] and act on the events.
///
/// Time is accumulated from the sample counts fed in, never read from a
/// clock. That keeps the whole thing deterministic and unit-testable without
/// sleeping, and sidesteps a real bug in Natively's dead `vad.rs`: it measures
/// hangover with wall-clock `SystemTime`, which is non-monotonic, so an NTP
/// correction stepping the clock backwards underflows its unsigned
/// subtraction and panics in debug builds.
pub struct Vad {
    config: VadConfig,
    state: VadState,
    /// Running estimate of the room's background level.
    noise_floor: f32,
    noise_initialized: bool,
    /// Total audio seen, in milliseconds. The timeline all events refer to.
    elapsed_ms: u64,
    /// Voiced milliseconds in the current utterance. Distinct from wall-clock
    /// duration: quiet moments inside an utterance don't count toward it.
    /// Used for the `min_utterance_ms` "was this real speech" check on a
    /// natural end — deliberately *not* used for the `max_utterance_ms`
    /// forced-cut check, which needs actual wall-clock time since that cap
    /// exists to bound buffer growth, not voiced content (see
    /// `step_speaking`'s own comment on that check).
    speech_ms: u32,
    /// Consecutive low milliseconds since the last voiced frame.
    silence_ms: u32,
    /// Consecutive above-threshold frames while still idle.
    consecutive_voiced: u32,
    /// When the current utterance began, already backdated.
    speech_started_at_ms: u64,
    /// True when the current segment continues one that a forced cut ended,
    /// meaning its audio is already-confirmed speech rather than a candidate
    /// cough. Without this, a monologue that stops shortly after a forced cut
    /// reports its tail as `NoiseDiscarded` and never emits `SpeechEnd`.
    segment_is_continuation: bool,
    /// Every state transition during the current batch, in order.
    ///
    /// A caller can't just compare state before and after `push_samples`: one
    /// oversized buffer can hold a whole Idle→Speech→Hangover→Idle cycle,
    /// which reads as "nothing happened" and leaves the speaking indicator
    /// dark through an entire utterance. Recording each transition means the
    /// caller can announce all of them, not just the net result.
    ///
    /// Capped at [`Vad::MAX_PENDING_TRANSITIONS`] so a caller that never
    /// drains can't leak. The `bool` this replaced couldn't grow at all, so
    /// the bound is what keeps the swap from being a downgrade.
    transitions: Vec<VadState>,
    /// Leftover mono samples from a previous call that didn't fill a frame.
    partial: Vec<f32>,
}

impl Vad {
    /// Upper bound on undrained transitions. Far above what any single audio
    /// buffer can produce (a 4096-sample buffer at 48 kHz is ~4 frames, so at
    /// most ~4 transitions), so a caller draining every batch — as `pump_vad`
    /// does — never approaches it. Once full, the oldest entries are dropped:
    /// a stale transition is worth less than a recent one, and the alternative
    /// is unbounded growth in a struct that lives on the audio path.
    pub const MAX_PENDING_TRANSITIONS: usize = 256;

    pub fn new(config: VadConfig) -> Self {
        Self {
            config: config.sanitized(),
            state: VadState::Idle,
            noise_floor: 0.0,
            noise_initialized: false,
            elapsed_ms: 0,
            speech_ms: 0,
            silence_ms: 0,
            consecutive_voiced: 0,
            speech_started_at_ms: 0,
            segment_is_continuation: false,
            transitions: Vec::new(),
            partial: Vec::new(),
        }
    }

    pub fn state(&self) -> VadState {
        self.state
    }

    pub fn noise_floor(&self) -> f32 {
        self.noise_floor
    }

    /// Takes every state transition recorded since the last call, in order.
    ///
    /// Callers should announce all of them rather than only the current
    /// state — see [`Vad::transitions`] for why the net result isn't enough.
    pub fn take_transitions(&mut self) -> Vec<VadState> {
        std::mem::take(&mut self.transitions)
    }

    /// Current entry threshold — exposed for the UI, so a level meter can
    /// draw where "speech" begins instead of hardcoding a guess.
    pub fn enter_threshold(&self) -> f32 {
        (self.noise_floor * self.config.enter_multiplier).max(self.config.absolute_floor)
    }

    /// Kept strictly below [`Vad::enter_threshold`] in both regimes.
    ///
    /// The floor term preserves the enter/exit ratio instead of reusing
    /// `absolute_floor` directly — a plain `.max(absolute_floor)` here would
    /// make the two thresholds *equal* once the noise floor is low enough,
    /// collapsing the hysteresis and chattering on every frame near the
    /// boundary.
    fn exit_threshold(&self) -> f32 {
        let ratio_floor =
            self.config.absolute_floor * self.config.exit_multiplier / self.config.enter_multiplier;
        (self.noise_floor * self.config.exit_multiplier).max(ratio_floor)
    }

    /// Feed captured audio. Splits into fixed-size analysis frames regardless
    /// of the buffer size the audio device happens to deliver, so behavior
    /// doesn't change when the hardware changes its buffer.
    ///
    /// `channels` must be the device's real channel count — cpal delivers
    /// **interleaved** samples, so a stereo device packs two samples per unit
    /// of real time. Treating that as mono makes every timer in this module
    /// run at 2× speed: a 500 ms hangover becomes 250 ms (clipping exactly the
    /// trailing consonants it exists to protect), a 200 ms noise guard becomes
    /// 100 ms, and the 15 s cap becomes 7.5 s. Stereo input is common on
    /// USB interfaces, aggregate devices and webcams, so this is not an edge
    /// case.
    ///
    /// Aside's own mic path pre-downmixes (so the level meter and this module
    /// measure the same signal) and therefore always passes `1`. The parameter
    /// is kept, exercised by tests, for any future caller that hands over raw
    /// device buffers — system-audio capture in particular. Don't remove it as
    /// dead just because the one current caller passes a constant.
    ///
    /// Returns boundary events. **Never returns audio** — see the module docs.
    pub fn push_samples(
        &mut self,
        interleaved: &[f32],
        sample_rate: u32,
        channels: u16,
    ) -> Vec<VadEvent> {
        let mut events = Vec::new();
        if sample_rate == 0 {
            return events;
        }
        let frame_len = ((sample_rate as u64 * self.config.frame_ms as u64) / 1000) as usize;
        if frame_len == 0 {
            return events;
        }

        super::pcm::downmix_to_mono(interleaved, channels, &mut self.partial);

        while self.partial.len() >= frame_len {
            let frame_rms = rms_level(&self.partial[..frame_len]);
            self.partial.drain(..frame_len);
            if let Some(event) = self.push_level(frame_rms, self.config.frame_ms) {
                events.push(event);
            }
        }
        events
    }

    /// The state machine proper, driven by one frame's loudness.
    ///
    /// Split out from [`Vad::push_samples`] so tests can drive exact levels
    /// and durations without synthesizing waveforms.
    pub fn push_level(&mut self, rms: f32, frame_ms: u32) -> Option<VadEvent> {
        // A single non-finite frame used to be permanently fatal: NaN flows
        // into the noise-floor average and every later value stays NaN, with
        // no reseed path. It fails quietly rather than loudly — `f32::max`
        // returns the non-NaN side, so thresholds silently pin to their
        // absolute floors and adaptation is dead for the rest of the session
        // with nothing to show for it. Infinity is the mirror image: it
        // compares as voiced and fakes a speech detection. Drop both.
        // Dropping the frame from *analysis* is not gating — the audio itself
        // is untouched and has already gone wherever it was going.
        if !rms.is_finite() {
            return None;
        }

        self.elapsed_ms += frame_ms as u64;

        // Seed the noise floor from the first frame, but never above the
        // configured floor. Natively's equivalent seeds unclamped; OpenCluely
        // clamps and explains why: if the very first frame is loud (the user
        // was already mid-sentence when capture started), an unclamped seed
        // pushes the entry threshold permanently out of reach and the
        // detector never fires again for the whole session.
        if !self.noise_initialized {
            self.noise_floor = rms.min(self.config.absolute_floor);
            self.noise_initialized = true;
        }

        let voiced = if self.state.is_speaking() {
            rms >= self.exit_threshold()
        } else {
            rms >= self.enter_threshold()
        };

        let before = self.state;
        let event = match self.state {
            VadState::Idle => self.step_idle(voiced, rms, frame_ms),
            VadState::Speech | VadState::Hangover => self.step_speaking(voiced, frame_ms),
        };
        if self.state != before {
            if self.transitions.len() >= Self::MAX_PENDING_TRANSITIONS {
                self.transitions.remove(0);
            }
            self.transitions.push(self.state);
        }
        event
    }

    fn step_idle(&mut self, voiced: bool, rms: f32, frame_ms: u32) -> Option<VadEvent> {
        if !voiced {
            self.consecutive_voiced = 0;
            self.adapt_noise_floor(rms, frame_ms);
            return None;
        }

        self.consecutive_voiced = self.consecutive_voiced.saturating_add(1);
        if self.consecutive_voiced < self.config.onset_frames {
            return None;
        }

        self.state = VadState::Speech;
        // Credit the frames that confirmed onset — they were real speech,
        // they just hadn't been declared yet.
        self.speech_ms = self.consecutive_voiced.saturating_mul(frame_ms);
        self.silence_ms = 0;
        self.consecutive_voiced = 0;
        self.segment_is_continuation = false;
        self.speech_started_at_ms = self
            .elapsed_ms
            .saturating_sub(self.speech_ms as u64)
            .saturating_sub(self.config.preroll_ms as u64);

        Some(VadEvent::SpeechStart {
            at_ms: self.speech_started_at_ms,
        })
    }

    fn step_speaking(&mut self, voiced: bool, frame_ms: u32) -> Option<VadEvent> {
        if voiced {
            self.state = VadState::Speech;
            self.speech_ms = self.speech_ms.saturating_add(frame_ms);
            self.silence_ms = 0;
        } else {
            self.state = VadState::Hangover;
            self.silence_ms = self.silence_ms.saturating_add(frame_ms);
        }

        // Force a cut before checking for a natural end, so a monologue that
        // hits the cap gets a boundary rather than being held indefinitely.
        //
        // Measured against *wall-clock* time since the segment/continuation
        // started (`elapsed_ms - speech_started_at_ms`), not `speech_ms`
        // (voiced-only accumulated time, which deliberately excludes quiet
        // moments inside an utterance — see its own field doc comment).
        // `max_utterance_ms` exists so the speaking-light cannot stay on
        // forever during a long monologue (the UI still gets a ForcedBoundary).
        // It no longer cuts STT — every live engine has its own endpointing.
        // The cap is measured in wall-clock time, not voiced-only time. A
        // voiced-only counter here let a segment with lots of borderline, near-threshold
        // audio — found in real testing: the mic picking up the computer's
        // own speaker output, never quite reaching a full hangover pause but
        // often dipping just under the exit threshold — accumulate real
        // wall-clock *minutes* of buffered audio before the cap ever fired,
        // since only a fraction of its frames counted as "voiced" (see
        // `docs/evidence.md`).
        let elapsed_in_segment = self.elapsed_ms.saturating_sub(self.speech_started_at_ms);
        if elapsed_in_segment >= self.config.max_utterance_ms as u64 {
            let at_ms = self.elapsed_ms;
            // Stay in Speech: the speaker hasn't stopped, only the segment
            // has. Resetting to Idle here would force the next frame to clear
            // the *entry* threshold mid-word.
            self.state = VadState::Speech;
            self.speech_ms = 0;
            self.silence_ms = 0;
            // Everything after this cut is a continuation of confirmed
            // speech, so it must not be judged against `min_utterance_ms` —
            // otherwise a monologue that stops just after a cut reports its
            // tail as noise and never emits a `SpeechEnd` at all.
            self.segment_is_continuation = true;
            self.speech_started_at_ms = at_ms;
            return Some(VadEvent::ForcedBoundary { at_ms });
        }

        if self.silence_ms < self.config.hangover_ms {
            return None;
        }

        let voiced_ms = self.speech_ms;
        let real_speech = self.segment_is_continuation || voiced_ms >= self.config.min_utterance_ms;
        let started_at_ms = self.speech_started_at_ms;
        // The hangover wait is detection latency, not speech. Report when the
        // speaker actually stopped.
        let at_ms = self
            .elapsed_ms
            .saturating_sub(self.config.hangover_ms as u64)
            .max(started_at_ms);

        self.state = VadState::Idle;
        self.speech_ms = 0;
        self.silence_ms = 0;
        self.consecutive_voiced = 0;
        self.segment_is_continuation = false;

        if real_speech {
            Some(VadEvent::SpeechEnd {
                started_at_ms,
                at_ms,
                voiced_ms,
            })
        } else {
            Some(VadEvent::NoiseDiscarded { voiced_ms })
        }
    }

    /// Exponential moving average of the background level.
    ///
    /// Called from the idle path only — structurally, not by an `if`, so it
    /// cannot be reached while the user is talking. That matters: a floor that
    /// adapts during speech climbs toward the speaker's own level and then
    /// cuts them off mid-sentence. Both references get this right and both
    /// call it out explicitly, and an attempt to add a carefully-guarded
    /// exception here reproduced exactly that failure — see the KNOWN
    /// LIMITATION note above `VadConfig`.
    fn adapt_noise_floor(&mut self, rms: f32, frame_ms: u32) {
        let alpha =
            (1.0 - (-(frame_ms as f32) / self.config.noise_floor_tau_ms).exp()).clamp(0.0, 1.0);
        self.noise_floor = self.noise_floor * (1.0 - alpha) + rms * alpha;
        debug_assert!(
            self.noise_floor.is_finite(),
            "noise floor went non-finite — a garbage frame reached adaptation"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: u32 = 20;
    const MONO: u16 = 1;

    /// A level comfortably above any plausible threshold.
    const LOUD: f32 = 0.2;
    /// Effectively silence.
    const QUIET: f32 = 0.0001;

    fn vad() -> Vad {
        Vad::new(VadConfig::default())
    }

    /// Settle the noise floor with silence so thresholds are in a known,
    /// realistic state before the behavior under test.
    fn settle(v: &mut Vad, frames: u32) {
        for _ in 0..frames {
            v.push_level(QUIET, FRAME);
        }
    }

    fn feed(v: &mut Vad, level: f32, frames: u32) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for _ in 0..frames {
            if let Some(e) = v.push_level(level, FRAME) {
                events.push(e);
            }
        }
        events
    }

    #[test]
    fn starts_idle_and_silence_keeps_it_idle() {
        let mut v = vad();
        let events = feed(&mut v, QUIET, 50);
        assert_eq!(v.state(), VadState::Idle);
        assert!(events.is_empty());
    }

    #[test]
    fn loud_audio_triggers_speech_start() {
        let mut v = vad();
        settle(&mut v, 50);
        let events = feed(&mut v, LOUD, 10);
        assert_eq!(v.state(), VadState::Speech);
        assert!(matches!(events.first(), Some(VadEvent::SpeechStart { .. })));
    }

    #[test]
    fn onset_requires_consecutive_frames_so_one_spike_is_not_speech() {
        let mut v = vad();
        settle(&mut v, 50);
        // A single loud frame — a key click — followed by silence.
        let events = feed(&mut v, LOUD, 1);
        assert!(events.is_empty(), "one frame must not open an utterance");
        assert_eq!(v.state(), VadState::Idle);
    }

    #[test]
    fn speech_start_is_backdated_by_preroll() {
        let cfg = VadConfig::default();
        let mut v = vad();
        settle(&mut v, 50);
        let elapsed_before = v.elapsed_ms;
        // Derived from config rather than hardcoded, so changing the default
        // onset confirmation doesn't silently break this test's arithmetic.
        let events = feed(&mut v, LOUD, cfg.onset_frames);

        let VadEvent::SpeechStart { at_ms } = events[0] else {
            panic!("expected SpeechStart, got {:?}", events[0]);
        };
        let expected = elapsed_before - cfg.preroll_ms as u64;
        assert_eq!(at_ms, expected);
        assert!(at_ms < elapsed_before, "start must predate detection");
    }

    #[test]
    fn hysteresis_keeps_utterance_alive_through_a_mid_sentence_dip() {
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 20);
        assert_eq!(v.state(), VadState::Speech);

        // A level between the exit and entry thresholds: not loud enough to
        // *start* speech, but must not end it either.
        let between = (v.enter_threshold() + v.exit_threshold()) / 2.0;
        assert!(between < v.enter_threshold() && between > v.exit_threshold());

        let events = feed(&mut v, between, 5);
        assert!(events.is_empty(), "a dip must not end the utterance");
        assert!(v.state().is_speaking());
    }

    #[test]
    fn exit_threshold_stays_strictly_below_enter_threshold_at_every_noise_level() {
        // The property that makes hysteresis work at all. Checked across the
        // whole range including the clamped regime, where a naive
        // `.max(absolute_floor)` on the exit side would make them equal.
        for floor in [0.0, 1e-9, 0.0001, 0.008, 0.05, 0.5, 1.0] {
            let mut v = vad();
            v.noise_floor = floor;
            assert!(
                v.exit_threshold() < v.enter_threshold(),
                "hysteresis inverted at noise_floor={floor}: exit={} enter={}",
                v.exit_threshold(),
                v.enter_threshold()
            );
        }
    }

    #[test]
    fn brief_pause_shorter_than_hangover_does_not_end_speech() {
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 30);

        // 300ms of quiet, well under the 500ms hangover.
        let events = feed(&mut v, QUIET, 15);
        assert!(events.is_empty());
        assert_eq!(v.state(), VadState::Hangover);
        assert!(v.state().is_speaking(), "hangover still shows as speaking");
    }

    #[test]
    fn sustained_silence_ends_speech_after_hangover() {
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 30);
        let events = feed(&mut v, QUIET, 40);

        assert_eq!(v.state(), VadState::Idle);
        let Some(VadEvent::SpeechEnd { voiced_ms, .. }) = events.first() else {
            panic!("expected SpeechEnd, got {events:?}");
        };
        assert_eq!(*voiced_ms, 30 * FRAME);
    }

    #[test]
    fn speech_end_timestamps_are_self_consistent() {
        // `at_ms - started_at_ms` must be a real span that contains
        // `voiced_ms`, with the hangover wait excluded. An earlier revision
        // reported `at_ms` at hangover *expiry* against a voiced-only
        // duration, so the obvious `at_ms - duration` was up to a second off.
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 20);
        feed(&mut v, QUIET, 10); // 200ms internal pause, under hangover
        feed(&mut v, LOUD, 20);
        let events = feed(&mut v, QUIET, 40);

        let Some(VadEvent::SpeechEnd {
            started_at_ms,
            at_ms,
            voiced_ms,
        }) = events.first()
        else {
            panic!("expected SpeechEnd, got {events:?}");
        };

        let span = at_ms - started_at_ms;
        assert!(
            span >= *voiced_ms as u64,
            "span {span} must contain voiced time {voiced_ms}"
        );
        // The internal pause is inside the span but not counted as voiced.
        assert!(
            span > *voiced_ms as u64,
            "span should exceed voiced time when there was an internal pause"
        );
    }

    #[test]
    fn short_burst_is_discarded_as_noise_not_reported_as_speech() {
        let mut v = vad();
        settle(&mut v, 50);
        // 5 frames = 100ms, under the 200ms minimum.
        feed(&mut v, LOUD, 5);
        let events = feed(&mut v, QUIET, 40);

        let Some(VadEvent::NoiseDiscarded { voiced_ms }) = events.first() else {
            panic!("expected NoiseDiscarded, got {events:?}");
        };
        assert_eq!(*voiced_ms, 5 * FRAME);
        assert_eq!(v.state(), VadState::Idle);
    }

    #[test]
    fn utterance_at_exactly_the_minimum_counts_as_speech() {
        let mut v = vad();
        settle(&mut v, 50);
        let min_frames = VadConfig::default().min_utterance_ms / FRAME;
        feed(&mut v, LOUD, min_frames);
        let events = feed(&mut v, QUIET, 40);
        assert!(
            matches!(events.first(), Some(VadEvent::SpeechEnd { .. })),
            "boundary case must be inclusive, got {events:?}"
        );
    }

    #[test]
    fn long_monologue_is_force_split_without_dropping_out_of_speech() {
        let mut v = vad();
        settle(&mut v, 50);
        let cap_frames = VadConfig::default().max_utterance_ms / FRAME;
        let events = feed(&mut v, LOUD, cap_frames + 5);

        assert!(
            events
                .iter()
                .any(|e| matches!(e, VadEvent::ForcedBoundary { .. })),
            "expected a forced boundary, got {events:?}"
        );
        // The critical property: still speaking after the cut, so the next
        // frame doesn't have to re-clear the higher entry threshold mid-word.
        assert_eq!(v.state(), VadState::Speech);
    }

    #[test]
    fn monologue_ending_just_after_a_forced_cut_still_reports_speech_end() {
        // The tail after a forced cut is confirmed speech, not a candidate
        // cough — judging it against `min_utterance_ms` made a 15s monologue
        // emit SpeechStart, ForcedBoundary, NoiseDiscarded{0} and never a
        // SpeechEnd at all.
        let mut v = vad();
        settle(&mut v, 50);
        let cap_frames = VadConfig::default().max_utterance_ms / FRAME;
        let mut all = feed(&mut v, LOUD, cap_frames);
        all.extend(feed(&mut v, QUIET, 40));

        assert!(
            all.iter()
                .any(|e| matches!(e, VadEvent::ForcedBoundary { .. })),
            "expected a forced boundary, got {all:?}"
        );
        assert!(
            all.iter().any(|e| matches!(e, VadEvent::SpeechEnd { .. })),
            "a 15s monologue must still close with SpeechEnd, got {all:?}"
        );
        assert!(
            !all.iter()
                .any(|e| matches!(e, VadEvent::NoiseDiscarded { .. })),
            "genuine speech must never be reported as noise, got {all:?}"
        );
    }

    #[test]
    fn noise_floor_does_not_adapt_during_a_pause_inside_speech() {
        // The claim is "adaptation is idle-only". Feeding only loud audio
        // can't prove it — the code path is unreachable by construction, so
        // the test would pass with adaptation deleted. Quiet frames during
        // Hangover are the case that actually distinguishes the two.
        //
        // The pause level must also *differ* from the settled floor: an
        // earlier revision paused at the same value it had settled at, and an
        // average toward a value it already holds cannot move — so that
        // version was equally tautological, just less obviously.
        let settle_at = QUIET;
        let pause_at = 0.003; // clearly different, still below entry threshold

        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 30);
        let floor_before = v.noise_floor();
        assert!(
            (floor_before - pause_at).abs() > 1e-4,
            "guard: pause level must differ from the settled floor or this proves nothing"
        );
        assert!(
            pause_at < v.enter_threshold(),
            "guard: pause must read as quiet"
        );
        let _ = settle_at;

        feed(&mut v, pause_at, 10); // quiet, but still inside the utterance
        assert_eq!(v.state(), VadState::Hangover);
        assert_eq!(
            v.noise_floor(),
            floor_before,
            "noise floor adapted during a mid-utterance pause"
        );
    }

    #[test]
    fn noise_floor_adapts_upward_in_a_noisy_room() {
        let mut v = vad();
        let quiet_room = 0.001;
        settle(&mut v, 10);
        let initial = v.noise_floor();

        // Room noise rises but stays below the absolute floor, so it must not
        // register as speech — it should just move the estimate.
        feed(&mut v, quiet_room, 500);
        assert!(
            v.noise_floor() > initial,
            "floor should track a noisier room: {initial} -> {}",
            v.noise_floor()
        );
        assert_eq!(v.state(), VadState::Idle);
    }

    #[test]
    fn a_long_continuous_speaker_is_never_cut_off_or_lost() {
        // Regression test for a reverted "fix". An escape hatch for steady
        // room noise adapted the floor upward during long unbroken speech; a
        // person answering an open-ended question tripped it in about a
        // minute, the floor climbed onto their own voice, and the detector
        // both cut them off and then went deaf to them entirely.
        //
        // This pins the property that matters more than the noise case: a
        // person who simply keeps talking must keep being detected.
        let mut v = vad();
        settle(&mut v, 50);
        let speaker = 0.2;

        // Five minutes of continuous speech with realistic sub-hangover
        // breaths, which deliberately do NOT end the utterance.
        for _ in 0..75 {
            feed(&mut v, speaker, 200); // 4s of speech
            feed(&mut v, QUIET, 20); // 400ms breath, under the 500ms hangover
        }

        assert!(
            v.state().is_speaking(),
            "a continuously talking person stopped being detected"
        );
        assert!(
            v.enter_threshold() < speaker,
            "threshold climbed onto the speaker's own level: enter={} speaker={speaker}",
            v.enter_threshold()
        );
    }

    #[test]
    fn fluctuating_near_threshold_audio_still_hits_the_cap_within_wall_clock_time() {
        // Regression test for a real bug found in live testing (not a
        // synthetic worry): audio that hovers around the exit threshold —
        // exactly what a microphone picking up its own computer's speaker
        // output looks like, never fully silent, never fully steady —
        // repeatedly dips into Hangover without ever completing a full
        // `hangover_ms` pause (so no real SpeechEnd), and each dip's frames
        // don't count toward the voiced-only `speech_ms` accumulator. Before
        // the wall-clock fix, that meant `max_utterance_ms`'s cap could take
        // several times longer than 15 real seconds to fire, producing one
        // enormous buffered segment instead of periodic bounded cuts.
        let mut v = vad();
        settle(&mut v, 50);

        // Alternate 300ms loud / 300ms just-under-threshold, well under the
        // 500ms hangover each time — so `state` never leaves Speech/Hangover
        // and a real SpeechEnd never fires, but roughly half of each cycle's
        // frames are "not voiced" and wouldn't count toward `speech_ms`.
        let just_under_exit = v.exit_threshold() * 0.5;
        let mut all_events = Vec::new();
        let max_ms = VadConfig::default().max_utterance_ms as u64;
        let mut elapsed_ms: u64 = 0;
        while elapsed_ms < max_ms + 2_000 {
            all_events.extend(feed(&mut v, LOUD, 15)); // 300ms
            all_events.extend(feed(&mut v, just_under_exit, 15)); // 300ms
            elapsed_ms += 600;
            if all_events
                .iter()
                .any(|e| matches!(e, VadEvent::ForcedBoundary { .. }))
            {
                break;
            }
        }

        assert!(
            all_events
                .iter()
                .any(|e| matches!(e, VadEvent::ForcedBoundary { .. })),
            "fluctuating near-threshold audio never hit the forced-boundary cap within {}ms + slack",
            max_ms
        );
        assert!(
            elapsed_ms <= max_ms + 1_000,
            "cap fired {elapsed_ms}ms in — should fire within ~{max_ms}ms of wall-clock time, not several times later"
        );
    }

    #[test]
    fn steady_loud_room_noise_pins_the_detector_a_known_accepted_limitation() {
        // Documents current behavior rather than asserting it's desirable.
        // Steady noise above the entry threshold holds the detector in Speech
        // indefinitely, because the floor only adapts while idle. The fix for
        // this was implemented and reverted — it could not distinguish a fan
        // from a person and broke the far more important case above. See the
        // KNOWN LIMITATION note above `VadConfig`.
        //
        // If a future change makes this test fail, that is progress: delete it
        // and keep the fix, provided `a_long_continuous_speaker_is_never_cut_off_or_lost`
        // still passes.
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, 0.05, 6_000); // two minutes of unbroken machine noise

        assert!(
            v.state().is_speaking(),
            "known limitation no longer reproduces — see this test's comment"
        );
    }

    #[test]
    fn a_single_nan_frame_does_not_permanently_kill_adaptation() {
        // NaN in the moving average poisons it forever, and does so silently:
        // `f32::max` returns the non-NaN operand, so thresholds pin to their
        // absolute floors and adaptation is simply dead with no error.
        let mut v = vad();
        settle(&mut v, 50);
        let before = v.noise_floor();

        v.push_level(f32::NAN, FRAME);
        assert!(v.noise_floor().is_finite(), "NaN reached the noise floor");
        assert_eq!(v.noise_floor(), before, "a garbage frame changed the floor");

        // And adaptation still works afterward.
        feed(&mut v, 0.001, 500);
        assert!(v.noise_floor() > before);
        assert!(v.noise_floor().is_finite());
    }

    #[test]
    fn an_infinite_frame_does_not_fake_a_speech_detection() {
        // +inf compares as voiced, so without a guard it opens an utterance.
        let mut v = vad();
        settle(&mut v, 50);
        let events = feed(&mut v, f32::INFINITY, 10);
        assert!(events.is_empty(), "infinity faked speech: {events:?}");
        assert_eq!(v.state(), VadState::Idle);
    }

    #[test]
    fn non_finite_frames_do_not_advance_the_clock() {
        let mut v = vad();
        v.push_level(f32::NAN, FRAME);
        v.push_level(f32::INFINITY, FRAME);
        v.push_level(f32::NEG_INFINITY, FRAME);
        assert_eq!(v.elapsed_ms, 0);
    }

    #[test]
    fn entry_threshold_never_falls_below_the_absolute_floor() {
        // Asserting `>= absolute_floor` alone would restate the `.max()` in
        // the implementation and pass for any noise floor at all. The real
        // property is that true digital silence doesn't decay the floor to
        // zero and make every faint sound read as speech — so check the
        // threshold is still *exactly* the floor, and that silence still
        // can't trigger detection.
        let mut v = vad();
        for _ in 0..1_000 {
            v.push_level(0.0, FRAME);
        }
        let cfg = VadConfig::default();
        assert_eq!(v.enter_threshold(), cfg.absolute_floor);
        assert_eq!(v.state(), VadState::Idle);
    }

    #[test]
    fn loud_first_frame_does_not_permanently_disable_detection() {
        // Capture starting mid-sentence: the very first frame is already
        // loud. An unclamped noise-floor seed would set the threshold above
        // anything the user can produce and the detector would never fire.
        let mut v = vad();
        v.push_level(LOUD, FRAME);
        settle(&mut v, 50);
        let events = feed(&mut v, LOUD, 10);
        assert!(
            matches!(events.first(), Some(VadEvent::SpeechStart { .. })),
            "detector stalled after a loud first frame, got {events:?}"
        );
    }

    #[test]
    fn adaptation_rate_is_independent_of_frame_size() {
        // The same wall-clock duration of identical audio must move the floor
        // to the same place whether it arrives as few big frames or many
        // small ones. Both references hardcode a per-chunk smoothing factor
        // and silently change behavior when the audio buffer size changes.
        //
        // The level must differ from the seed, or the EMA has nothing to
        // travel toward and the test passes for any alpha at all — including
        // alpha 0, i.e. with adaptation deleted.
        let settle_at = 0.0005;
        let then = 0.004;
        let total_ms = 2_000;

        let mut small = vad();
        let mut large = vad();
        small.push_level(settle_at, 10);
        large.push_level(settle_at, 40);

        for _ in 0..(total_ms / 10) {
            small.push_level(then, 10);
        }
        for _ in 0..(total_ms / 40) {
            large.push_level(then, 40);
        }

        assert!(
            small.noise_floor() > settle_at * 1.5,
            "guard: the floor must actually have moved, got {}",
            small.noise_floor()
        );
        let delta = (small.noise_floor() - large.noise_floor()).abs();
        assert!(
            delta < 1e-6,
            "frame size changed adaptation: 10ms -> {}, 40ms -> {}",
            small.noise_floor(),
            large.noise_floor()
        );
    }

    #[test]
    fn stereo_input_is_downmixed_so_timers_run_at_real_time() {
        // cpal delivers interleaved samples. Treating a stereo buffer as mono
        // makes every timer in the module run at 2x speed — a 500ms hangover
        // becomes 250ms, clipping exactly the trailing consonants it exists
        // to protect.
        let rate = 48_000;
        let one_second_stereo = vec![0.0_f32; (rate * 2) as usize];

        let mut v = vad();
        v.push_samples(&one_second_stereo, rate, 2);
        assert_eq!(v.elapsed_ms, 1_000, "one real second must read as 1000ms");
    }

    #[test]
    fn downmix_averages_channels_rather_than_interleaving_them() {
        let rate = 16_000;
        // Left channel loud, right channel silent -> mono average is half.
        let interleaved: Vec<f32> = (0..rate * 2)
            .map(|i| if i % 2 == 0 { 0.4 } else { 0.0 })
            .collect();

        let mut v = vad();
        v.push_samples(&interleaved, rate, 2);
        assert_eq!(v.elapsed_ms, 1_000);
        // 0.4 on one of two channels averages to 0.2, well above threshold.
        assert!(v.state().is_speaking(), "downmixed speech was lost");
    }

    #[test]
    fn push_samples_splits_arbitrary_buffers_into_fixed_frames() {
        let mut v = vad();
        let rate = 48_000;
        // 100ms of silence delivered as one oversized buffer.
        let buf = vec![0.0_f32; (rate / 10) as usize];
        v.push_samples(&buf, rate, MONO);
        // 5 frames of 20ms should have been consumed, with none left over.
        assert_eq!(v.elapsed_ms, 100);
        assert!(v.partial.is_empty());
    }

    #[test]
    fn push_samples_carries_partial_frames_across_calls() {
        let mut v = vad();
        let rate = 48_000;
        let frame_len = (rate / 50) as usize; // 20ms
                                              // Deliver one and a half frames.
        v.push_samples(&vec![0.0_f32; frame_len + frame_len / 2], rate, MONO);
        assert_eq!(v.elapsed_ms, 20, "only one whole frame should be consumed");
        assert_eq!(v.partial.len(), frame_len / 2);

        // The other half completes the second frame.
        v.push_samples(&vec![0.0_f32; frame_len / 2], rate, MONO);
        assert_eq!(v.elapsed_ms, 40);
        assert!(v.partial.is_empty());
    }

    #[test]
    fn partial_buffer_stays_bounded_below_one_frame() {
        // If this could grow without bound it would be a memory leak in the
        // audio callback — and arguably a gate in disguise.
        let mut v = vad();
        let rate = 48_000;
        let frame_len = (rate / 50) as usize;
        for _ in 0..1_000 {
            v.push_samples(&vec![0.0_f32; frame_len + 7], rate, MONO);
        }
        assert!(
            v.partial.len() < frame_len,
            "partial buffer grew to {} (frame is {frame_len})",
            v.partial.len()
        );
    }

    #[test]
    fn push_samples_detects_real_speech_from_a_waveform() {
        // End-to-end through the sample path rather than injected levels.
        let mut v = vad();
        let rate = 16_000;
        let silence = vec![0.0_f32; rate as usize]; // 1s
        v.push_samples(&silence, rate, MONO);

        // A 1s square wave at 0.3 amplitude — unambiguously "speech" energy.
        let tone: Vec<f32> = (0..rate)
            .map(|i| if (i / 40) % 2 == 0 { 0.3 } else { -0.3 })
            .collect();
        let events = v.push_samples(&tone, rate, MONO);

        assert!(
            matches!(events.first(), Some(VadEvent::SpeechStart { .. })),
            "expected speech from a real waveform, got {events:?}"
        );
        assert!(v.state().is_speaking());
    }

    #[test]
    fn zero_sample_rate_is_ignored_rather_than_dividing_by_zero() {
        let mut v = vad();
        assert!(v.push_samples(&[0.1, 0.2], 0, MONO).is_empty());
        assert_eq!(v.elapsed_ms, 0);
    }

    #[test]
    fn zero_channels_is_treated_as_mono_rather_than_dividing_by_zero() {
        let mut v = vad();
        let rate = 16_000;
        v.push_samples(&vec![0.0_f32; rate as usize], rate, 0);
        assert_eq!(v.elapsed_ms, 1_000);
    }

    #[test]
    fn transitions_survive_a_full_speech_cycle_inside_one_buffer() {
        // A single oversized buffer really can contain an entire speech cycle,
        // so a caller comparing state before and after the call sees
        // Idle -> Idle and never lights the indicator. Driven through
        // `push_samples` with one large buffer, because that IS the scenario —
        // an earlier version of this test used per-frame calls, so the buffer
        // it was named for was never involved.
        let rate = 16_000;
        let mut v = vad();
        v.push_samples(&vec![0.0_f32; rate as usize], rate, 1); // settle
        v.take_transitions();

        // One ~2.5s buffer: 1s of speech then 1.5s of silence, i.e. a whole
        // Idle -> Speech -> Hangover -> Idle cycle in a single call.
        let mut buffer: Vec<f32> = (0..rate)
            .map(|i| if (i / 40) % 2 == 0 { 0.3 } else { -0.3 })
            .collect();
        buffer.extend(std::iter::repeat(0.0).take((rate as usize * 3) / 2));

        v.push_samples(&buffer, rate, 1);
        assert_eq!(v.state(), VadState::Idle, "cycle should have completed");

        let transitions = v.take_transitions();
        assert!(
            transitions.iter().any(|s| s.is_speaking()),
            "the speaking period must be reported, not just the idle end state: {transitions:?}"
        );
        assert_eq!(
            transitions.last(),
            Some(&VadState::Idle),
            "last transition should be back to idle: {transitions:?}"
        );
        assert!(
            v.take_transitions().is_empty(),
            "transitions must clear once taken"
        );
    }

    #[test]
    fn every_speech_start_is_eventually_terminated() {
        // The invariant a consumer depends on: once speech starts, it is
        // always told when it ends, whatever happened in between. Checked
        // across durations that straddle the forced-cut boundary, since that
        // is where an earlier revision dropped the terminator entirely.
        let cap_frames = VadConfig::default().max_utterance_ms / FRAME;
        for frames in [3, 10, 30, cap_frames - 1, cap_frames, cap_frames + 10] {
            let mut v = vad();
            settle(&mut v, 50);
            let mut all = feed(&mut v, LOUD, frames);
            all.extend(feed(&mut v, QUIET, 40));

            let started = all
                .iter()
                .any(|e| matches!(e, VadEvent::SpeechStart { .. }));
            let terminated = all.iter().any(|e| {
                matches!(
                    e,
                    VadEvent::SpeechEnd { .. } | VadEvent::NoiseDiscarded { .. }
                )
            });
            // Assert onset separately rather than folding it into an
            // implication. `!started || terminated` is satisfied whenever
            // nothing started, so on its own it would pass silently if a
            // change stopped speech being detected at all — and `state ==
            // Idle` doesn't catch that either, since Idle is also the
            // never-started state.
            assert!(started, "no speech detected at all after {frames} frames");
            assert!(
                terminated,
                "speech started but was never terminated after {frames} frames: {all:?}"
            );
            assert_eq!(v.state(), VadState::Idle, "left speaking after silence");
        }
    }

    #[test]
    fn a_full_utterance_produces_exactly_one_start_and_one_end() {
        let mut v = vad();
        settle(&mut v, 50);
        let mut all = Vec::new();
        all.extend(feed(&mut v, LOUD, 15));
        all.extend(feed(&mut v, QUIET, 10)); // pause, under hangover
        all.extend(feed(&mut v, LOUD, 15));
        all.extend(feed(&mut v, QUIET, 40)); // real end

        let starts = all
            .iter()
            .filter(|e| matches!(e, VadEvent::SpeechStart { .. }))
            .count();
        let ends = all
            .iter()
            .filter(|e| matches!(e, VadEvent::SpeechEnd { .. }))
            .count();
        assert_eq!((starts, ends), (1, 1), "events were {all:?}");
    }

    #[test]
    fn pathological_config_is_clamped_instead_of_breaking_the_state_machine() {
        // All fields are pub for tuning, so these are reachable. Each one
        // broke the machine differently before sanitizing: enter 0 divided by
        // zero, max 0 fired a boundary every single frame (50 IPC messages a
        // second off the audio thread), min > max made SpeechEnd unreachable.
        let cfg = VadConfig {
            frame_ms: 0,
            enter_multiplier: 0.0,
            exit_multiplier: 9.0,
            min_utterance_ms: 60_000,
            max_utterance_ms: 0,
            onset_frames: 0,
            ..Default::default()
        };
        let v = Vad::new(cfg);

        assert!(v.config.frame_ms >= 1);
        assert!(v.config.enter_multiplier > 0.0);
        assert!(v.config.exit_multiplier < v.config.enter_multiplier);
        assert!(v.config.min_utterance_ms <= v.config.max_utterance_ms);
        assert!(v.config.onset_frames >= 1);
        assert!(v.exit_threshold().is_finite());
        assert!(v.exit_threshold() < v.enter_threshold());
    }

    #[test]
    fn nan_config_fields_are_replaced_rather_than_propagated() {
        // `f32::clamp` propagates NaN while `f32::max` drops it, so NaN has to
        // be handled before any clamping or it survives some guards and not
        // others. A NaN exit_multiplier is the worst case: every comparison
        // against it is false, turning continuous speech into an endless
        // start/discard cycle — ~2 spurious utterances a second, each an IPC
        // message off the real-time audio thread.
        let cfg = VadConfig {
            noise_floor_tau_ms: f32::NAN,
            enter_multiplier: f32::NAN,
            exit_multiplier: f32::NAN,
            absolute_floor: f32::NAN,
            ..Default::default()
        };
        let mut v = Vad::new(cfg);

        assert!(v.config.exit_multiplier.is_finite());
        assert!(v.config.absolute_floor.is_finite());
        assert!(v.enter_threshold().is_finite());
        assert!(v.exit_threshold().is_finite());

        // And it behaves: one utterance in, one start and one end out.
        settle(&mut v, 50);
        let mut all = feed(&mut v, LOUD, 30);
        all.extend(feed(&mut v, QUIET, 40));
        let starts = all
            .iter()
            .filter(|e| matches!(e, VadEvent::SpeechStart { .. }))
            .count();
        assert_eq!(starts, 1, "NaN config caused an event storm: {all:?}");
    }

    #[test]
    fn undrained_transitions_stay_bounded() {
        // `pump_vad` drains every callback so production never accumulates,
        // but both push methods are public and nothing forces a drain. The
        // `bool` this replaced couldn't grow at all, so an unbounded Vec would
        // have been a regression.
        let mut v = vad();
        settle(&mut v, 50);
        feed(&mut v, LOUD, 5);
        for i in 0..20_000 {
            v.push_level(if i % 2 == 0 { LOUD } else { QUIET }, FRAME);
        }
        assert!(
            v.transitions.len() <= Vad::MAX_PENDING_TRANSITIONS,
            "transitions grew to {}",
            v.transitions.len()
        );
    }

    #[test]
    fn clamped_config_still_produces_a_normal_utterance() {
        // Clamping is only worth anything if the result actually works.
        let cfg = VadConfig {
            enter_multiplier: 0.0,
            exit_multiplier: 9.0,
            max_utterance_ms: 0,
            ..Default::default()
        }
        .sanitized();
        let mut v = Vad::new(cfg);
        settle(&mut v, 50);
        let events = feed(&mut v, LOUD, 20);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, VadEvent::SpeechStart { .. })),
            "clamped config never detected speech: {events:?}"
        );
    }
}
