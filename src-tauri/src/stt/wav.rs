//! Wraps raw mono `f32` samples in a WAV container.
//!
//! Live cloud sockets send raw PCM; this helper also wraps the same samples
//! in a 44-byte WAV header for any caller that needs a complete file.
//! `references/Repo1-Natively`'s `RestSTT.ts` does the same thing (raw PCM
//! + a hand-built WAV header) rather than pulling in a general-purpose
//! audio-encoding crate for a format this simple and this fixed.

/// Converts mono `f32` samples (expected range `[-1.0, 1.0]`) to little-endian
/// 16-bit PCM bytes. Out-of-range samples are clamped rather than wrapped —
/// a clipped-but-recognizable waveform beats aliasing noise.
///
/// Factored out of [`encode_mono_16bit`] so a raw streaming consumer (a
/// WebSocket sending live PCM frames, which needs no WAV header at all — the
/// format is already declared out-of-band via the connection's own params)
/// can share the exact same conversion instead of a second copy that could
/// drift from this one.
pub fn f32_to_i16_le_bytes(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let pcm = (clamped * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&pcm.to_le_bytes());
    }
    out
}

/// Encodes mono `f32` samples as 16-bit PCM WAV bytes at `sample_rate`.
#[cfg(test)]
pub fn encode_mono_16bit(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let pcm_bytes = f32_to_i16_le_bytes(samples);
    let data_len = pcm_bytes.len();
    let mut out = Vec::with_capacity(44 + data_len);

    let byte_rate = sample_rate * 2; // mono, 16-bit
    let block_align: u16 = 2;

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    out.extend_from_slice(&pcm_bytes);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_declares_the_right_riff_and_wave_tags() {
        let wav = encode_mono_16bit(&[0.0, 0.0], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }

    #[test]
    fn declares_mono_pcm_at_the_given_sample_rate() {
        let wav = encode_mono_16bit(&[0.0; 100], 48_000);
        let num_channels = u16::from_le_bytes([wav[22], wav[23]]);
        let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        let bits_per_sample = u16::from_le_bytes([wav[34], wav[35]]);
        assert_eq!(num_channels, 1);
        assert_eq!(sample_rate, 48_000);
        assert_eq!(bits_per_sample, 16);
    }

    #[test]
    fn data_chunk_size_matches_two_bytes_per_sample() {
        let samples = vec![0.1_f32; 500];
        let wav = encode_mono_16bit(&samples, 16_000);
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_size, 1000);
        assert_eq!(wav.len(), 44 + 1000);
    }

    #[test]
    fn riff_size_field_is_file_length_minus_eight() {
        let samples = vec![0.0_f32; 10];
        let wav = encode_mono_16bit(&samples, 16_000);
        let riff_size = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]);
        assert_eq!(riff_size as usize, wav.len() - 8);
    }

    #[test]
    fn full_scale_positive_and_negative_samples_do_not_wrap() {
        let wav = encode_mono_16bit(&[1.0, -1.0], 16_000);
        let s0 = i16::from_le_bytes([wav[44], wav[45]]);
        let s1 = i16::from_le_bytes([wav[46], wav[47]]);
        assert_eq!(s0, i16::MAX);
        // -1.0 * i16::MAX rounds to -32767, one shy of i16::MIN (-32768) —
        // correct: symmetric clamping around a MAX-based scale never reaches
        // MIN, and that is preferable to an asymmetric scale that could.
        assert_eq!(s1, -i16::MAX);
    }

    #[test]
    fn out_of_range_samples_are_clamped_not_wrapped() {
        let wav = encode_mono_16bit(&[2.5, -3.0], 16_000);
        let s0 = i16::from_le_bytes([wav[44], wav[45]]);
        let s1 = i16::from_le_bytes([wav[46], wav[47]]);
        assert_eq!(s0, i16::MAX, "must clamp to full scale, not overflow/wrap");
        assert_eq!(s1, -i16::MAX);
    }

    #[test]
    fn f32_to_i16_le_bytes_produces_two_bytes_per_sample_little_endian() {
        let bytes = f32_to_i16_le_bytes(&[1.0, -1.0]);
        assert_eq!(bytes.len(), 4);
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([bytes[2], bytes[3]]), -i16::MAX);
    }

    #[test]
    fn wav_encoding_and_raw_pcm_conversion_agree_on_the_same_samples() {
        // Regression guard for the shared-helper refactor: the WAV path's
        // data chunk must be byte-for-byte identical to the raw PCM helper's
        // output, not a second, independently-drifting conversion.
        let samples = vec![0.25, -0.75, 0.0, 1.0, -1.0];
        let raw = f32_to_i16_le_bytes(&samples);
        let wav = encode_mono_16bit(&samples, 16_000);
        assert_eq!(&wav[44..], raw.as_slice());
    }

    #[test]
    fn empty_input_produces_a_header_only_wav() {
        let wav = encode_mono_16bit(&[], 16_000);
        assert_eq!(wav.len(), 44);
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_size, 0);
    }
}
