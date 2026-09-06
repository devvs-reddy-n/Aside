//! Pure sample-math shared by every capture channel.
//!
//! Hoisted out of `mic.rs` when system-audio capture arrived: both channels
//! need the identical level math, and the one thing this codebase already
//! knows about duplicated audio paths is that they drift. `mic.rs`'s own
//! comments record Natively ending up with two near-identical audio loops
//! that fell out of sync; the fix is one copy, one test suite, both callers.
//!
//! Everything here is pure — no devices, no threads — so it is fully
//! testable without audio hardware, which is what makes the `i16` and
//! multi-channel paths verifiable at all on a machine that has neither.

/// Root-mean-square of a chunk of samples — the standard measure of how
/// loud that chunk is. Pure so it's testable without real audio hardware.
pub fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// Converts `i16` device samples to the ±1.0 range the rest of the pipeline
/// works in, appending to `out`.
///
/// Everything downstream — the level meter, the VAD thresholds — is calibrated
/// in normalized units, so an `i16` device has to be converted before any of
/// it applies. Extracted rather than inlined in the callback specifically so
/// the scale factor is covered by tests: this path has never run on real
/// `i16` hardware, so its correctness rests entirely on them.
pub fn normalize_i16_into(samples: &[i16], out: &mut Vec<f32>) {
    out.extend(samples.iter().map(|&s| s as f32 / 32768.0));
}

/// Collapses an interleaved multi-channel buffer to mono by averaging each
/// frame's channels, appending to `out`.
///
/// One implementation shared by the level meter and the detector, on purpose.
/// When they diverged — the meter measuring the raw interleaved buffer while
/// VAD measured the downmix — a stereo device with one dead channel (a mono
/// mic on a stereo interface, the most common stereo-input case) made the
/// meter read ~41% high against the very threshold line drawn beside it. The
/// user would watch the bar cross the line while the indicator stayed dark.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    // `chunks_exact` drops a trailing partial frame. cpal delivers whole
    // frames, so this only bites on malformed input — where discarding under
    // one sample-time is the right call anyway.
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_empty_is_zero() {
        assert_eq!(rms_level(&[]), 0.0);
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms_level(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn rms_of_constant_amplitude_equals_that_amplitude() {
        let samples = vec![0.5_f32; 100];
        assert!((rms_level(&samples) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rms_of_square_wave_ignores_sign() {
        let samples: Vec<f32> = (0..100)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!((rms_level(&samples) - 1.0).abs() < 1e-6);
    }

    fn normalized(samples: &[i16]) -> Vec<f32> {
        let mut out = Vec::new();
        normalize_i16_into(samples, &mut out);
        out
    }

    #[test]
    fn normalize_i16_maps_silence_to_zero() {
        assert_eq!(normalized(&[0, 0, 0]), vec![0.0, 0.0, 0.0]);
        assert!(normalized(&[]).is_empty());
    }

    #[test]
    fn normalize_i16_maps_full_scale_just_under_one() {
        // i16::MAX / 32768 is 0.99997, not 1.0 — the asymmetry is inherent to
        // two's complement and is far too small to matter at VAD thresholds,
        // but pinning it stops anyone "fixing" it to 32767 and shifting every
        // calibrated level by a hair.
        let out = normalized(&[i16::MAX, i16::MIN]);
        assert!((out[0] - 0.99997).abs() < 1e-4);
        assert_eq!(out[1], -1.0);
    }

    #[test]
    fn i16_and_f32_devices_produce_the_same_level_reading() {
        // The whole point of normalizing: an i16 device and an f32 device
        // carrying the same waveform must land on the same RMS, or one set of
        // VAD thresholds can't be correct for both. This is the only thing
        // standing behind the i16 path, which has never run on real hardware.
        let raw: [i16; 4] = [16384, -16384, 8192, -8192];
        let expected_f32: Vec<f32> = vec![0.5, -0.5, 0.25, -0.25];
        assert_eq!(normalized(&raw), expected_f32);
        assert!((rms_level(&normalized(&raw)) - rms_level(&expected_f32)).abs() < 1e-6);
    }

    #[test]
    fn normalize_i16_appends_rather_than_replacing() {
        // The audio callback reuses one buffer across calls, so a `clear()`
        // it forgot would silently concatenate frames.
        let mut out = vec![9.0_f32];
        normalize_i16_into(&[0], &mut out);
        assert_eq!(out, vec![9.0, 0.0]);
    }

    #[test]
    fn downmix_averages_stereo_to_mono() {
        let mut out = Vec::new();
        // Interleaved L/R: (0.4, 0.0) and (0.2, 0.6).
        downmix_to_mono(&[0.4, 0.0, 0.2, 0.6], 2, &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.2).abs() < 1e-6);
        assert!((out[1] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn downmix_passes_mono_through_untouched() {
        let mut out = Vec::new();
        downmix_to_mono(&[0.1, -0.2, 0.3], 1, &mut out);
        assert_eq!(out, vec![0.1, -0.2, 0.3]);
    }

    #[test]
    fn downmix_treats_zero_channels_as_mono_rather_than_dividing_by_zero() {
        let mut out = Vec::new();
        downmix_to_mono(&[0.1, 0.2], 0, &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn meter_and_detector_measure_the_same_signal_on_a_half_dead_stereo_device() {
        // A mono mic on a stereo interface — one channel carries audio, the
        // other is silent. When the meter measured the raw interleaved buffer
        // while VAD measured the downmix, the meter read ~41% high against the
        // threshold line drawn right beside it: the bar would visibly cross
        // the line while the indicator stayed dark.
        let interleaved: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 0.4 } else { 0.0 })
            .collect();
        let mut mono = Vec::new();
        downmix_to_mono(&interleaved, 2, &mut mono);

        let meter = rms_level(&mono);
        let raw = rms_level(&interleaved);
        assert!(
            (meter - 0.2).abs() < 1e-4,
            "downmixed level should be 0.2, got {meter}"
        );
        assert!(
            raw > meter * 1.3,
            "guard: the raw reading really is inflated ({raw} vs {meter}) — \
             this test is meaningless if they already agree"
        );
    }
}
