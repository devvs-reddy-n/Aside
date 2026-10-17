//! Resamples audio to the fixed 16kHz rate local and live STT engines need.
//!
//! Aside's capture devices commonly run at 48kHz (both `mic.rs` and
//! `system.rs` target 48kHz where the device allows it); Parakeet and the
//! live cloud sockets want mono `f32` at exactly 16kHz. Linear
//! interpolation, not a windowed-sinc resampler — the STT input pipeline
//! already applies a mel-spectrogram filterbank that smooths over the mild
//! aliasing linear interpolation introduces, and a simple, testable,
//! dependency-free resampler is the right trade for this project's scale
//! over pulling in a DSP crate for a fixed, known conversion.

pub const STT_SAMPLE_RATE: u32 = 16_000;

/// Causal, continuous conversion for live audio. The low-pass filter prevents
/// high-frequency input from folding into speech when reducing the rate.
/// History and fractional position survive callback boundaries (44.1kHz too).
pub struct StreamingResampler {
    from: u32,
    to: u32,
    taps: Vec<f32>,
    history: Vec<f32>,
    cursor: usize,
    input_index: u64,
    next_output: f64,
    previous: f32,
}

impl Default for StreamingResampler {
    fn default() -> Self {
        Self { from: 0, to: 0, taps: Vec::new(), history: Vec::new(), cursor: 0,
            input_index: 0, next_output: 0.0, previous: 0.0 }
    }
}

impl StreamingResampler {
    pub fn process(&mut self, input: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == 0 || to == 0 || input.is_empty() { return Vec::new(); }
        if self.from != from || self.to != to {
            *self = Self::default();
            self.from = from;
            self.to = to;
            if from > to {
                let count = 95;
                let center = (count - 1) as f64 / 2.0;
                let cutoff = 0.47 * to as f64 / from as f64;
                self.taps = (0..count).map(|i| {
                    let x = i as f64 - center;
                    let sinc = if x == 0.0 { 2.0 * cutoff } else {
                        (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
                    };
                    let phase = 2.0 * std::f64::consts::PI * i as f64 / (count - 1) as f64;
                    (sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())) as f32
                }).collect();
                let sum: f32 = self.taps.iter().sum();
                self.taps.iter_mut().for_each(|tap| *tap /= sum);
                self.history = vec![0.0; count];
            }
        }
        if from == to { return input.to_vec(); }
        let ratio = from as f64 / to as f64;
        let mut out = Vec::with_capacity((input.len() as f64 / ratio).ceil() as usize + 1);
        for &sample in input {
            let filtered = if self.taps.is_empty() { sample } else {
                self.history[self.cursor] = if sample.is_finite() { sample } else { 0.0 };
                let n = self.history.len();
                let value = self.taps.iter().enumerate().map(|(i, tap)| {
                    tap * self.history[(self.cursor + n - i) % n]
                }).sum();
                self.cursor = (self.cursor + 1) % n;
                value
            };
            let index = self.input_index as f64;
            while self.next_output <= index + 1e-8 {
                let fraction = (self.next_output - (index - 1.0)).clamp(0.0, 1.0) as f32;
                out.push(self.previous + (filtered - self.previous) * fraction);
                self.next_output += ratio;
            }
            self.previous = filtered;
            self.input_index += 1;
        }
        out
    }
}

/// Resamples `input` from `from_rate` to `to_rate` via linear interpolation.
///
/// Matches this codebase's established pattern of treating a degenerate rate
/// (zero) as a no-op rather than dividing by zero — see
/// `audio::vad`'s `zero_sample_rate_is_ignored_rather_than_dividing_by_zero`
/// for the precedent this follows.
pub fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate || from_rate == 0 || to_rate == 0 {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).round().max(0.0) as usize;
    let last = input.len() - 1;

    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = (src_pos.floor() as usize).min(last);
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx];
        let b = input[(idx + 1).min(last)];
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_conversion_is_identical_across_irregular_callback_boundaries() {
        for rate in [24_000, 44_100, 48_000] {
            let input: Vec<f32> = (0..rate).map(|i| (i as f32 * 0.13).sin()).collect();
            let expected = StreamingResampler::default().process(&input, rate, 16_000);
            let mut converter = StreamingResampler::default();
            let actual: Vec<f32> = input.chunks(137).flat_map(|c| converter.process(c, rate, 16_000)).collect();
            assert_eq!(actual, expected, "callback boundaries changed {rate}Hz audio");
            assert_eq!(actual.len(), 16_000);
        }
    }

    #[test]
    fn live_downsampling_preserves_speech_and_rejects_high_frequency_aliases() {
        let amplitude = |hz: f64| {
            let input: Vec<f32> = (0..48_000).map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0).sin() as f32).collect();
            let output = StreamingResampler::default().process(&input, 48_000, 16_000);
            (output[1600..].iter().map(|v| v*v).sum::<f32>() / (output.len()-1600) as f32).sqrt()
        };
        assert!(amplitude(1000.0) > 0.69);
        assert!(amplitude(6000.0) > 0.65);
        assert!(amplitude(10_000.0) < 0.005, "10kHz must not become a false 6kHz tone");
    }

    #[test]
    fn live_conversion_handles_a_device_rate_change_without_old_filter_history() {
        let mut converter = StreamingResampler::default();
        converter.process(&[1.0; 480], 48_000, 16_000);
        let switched = converter.process(&[0.0; 240], 24_000, 16_000);
        assert_eq!(switched.len(), 160);
        assert!(switched.iter().all(|v| *v == 0.0));
        assert_eq!(converter.process(&[0.25; 160], 16_000, 16_000), vec![0.25;160]);
    }

    #[test]
    fn same_rate_returns_the_input_unchanged() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn empty_input_stays_empty() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn zero_rates_are_treated_as_a_no_op_not_a_division_by_zero() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 0, 16_000), input);
        assert_eq!(resample_linear(&input, 48_000, 0), input);
    }

    #[test]
    fn downsampling_48k_to_16k_produces_roughly_a_third_the_samples() {
        let input = vec![0.0_f32; 4800]; // 100ms at 48kHz
        let out = resample_linear(&input, 48_000, 16_000);
        // Exactly a third for an integer ratio like 48000/16000 = 3.
        assert_eq!(out.len(), 1600);
    }

    #[test]
    fn upsampling_16k_to_48k_produces_roughly_triple_the_samples() {
        let input = vec![0.0_f32; 1600];
        let out = resample_linear(&input, 16_000, 48_000);
        assert_eq!(out.len(), 4800);
    }

    #[test]
    fn a_constant_signal_resamples_to_the_same_constant() {
        // The simplest possible correctness check: interpolating between two
        // identical values must return that value, not drift.
        let input = vec![0.5_f32; 4800];
        let out = resample_linear(&input, 48_000, 16_000);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn a_linear_ramp_resamples_to_the_matching_point_on_the_ramp() {
        // Interpolating a straight line should reproduce the same line at
        // the new sample positions — the one shape linear interpolation is
        // exactly correct for, not just approximately.
        let input: Vec<f32> = (0..4800).map(|i| i as f32 / 4800.0).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        for (i, &s) in out.iter().enumerate() {
            let expected = (i as f64 * 3.0) / 4800.0;
            assert!(
                (s as f64 - expected).abs() < 1e-3,
                "sample {i}: got {s}, expected ~{expected}"
            );
        }
    }

    #[test]
    fn does_not_read_past_the_end_of_the_input() {
        // A regression guard for the `idx + 1` lookahead: the last output
        // sample's interpolation must not index out of bounds.
        let input = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let out = resample_linear(&input, 48_000, 16_000);
        assert!(!out.is_empty());
    }
}
