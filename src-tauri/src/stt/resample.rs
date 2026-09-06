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
