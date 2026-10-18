//! A short local chime on the same output device that system audio captures.
//! No input is synthesized: the pre-call check must hear it through the real tap.
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn sample(frame: usize, rate: u32) -> f32 {
    let t = frame as f32 / rate as f32;
    let (start, frequency) = if t < 0.18 {
        (0.0, 660.0)
    } else if (0.26..0.44).contains(&t) {
        (0.26, 880.0)
    } else {
        return 0.0;
    };
    let phase = t - start;
    let fade = (phase / 0.008).min((0.18 - phase) / 0.008).clamp(0.0, 1.0);
    (phase * frequency * std::f32::consts::TAU).sin() * 0.08 * fade
}

fn play_typed<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let callbacks = Arc::new(AtomicUsize::new(0));
    let observed = callbacks.clone();
    let error = Arc::new(Mutex::new(None));
    let callback_error = error.clone();
    let callback_cancelled = cancelled.clone();
    let mut frame = 0;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                observed.fetch_add(1, Ordering::Relaxed);
                for output in data.chunks_mut(channels) {
                    let value = T::from_sample(if callback_cancelled.load(Ordering::Relaxed) {
                        0.0
                    } else {
                        sample(frame, rate)
                    });
                    output.fill(value);
                    frame += 1;
                }
            },
            move |err| {
                *callback_error.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(format!("Couldn't play the test sound: {err}"));
            },
            None,
        )
        .map_err(|err| format!("Couldn't open your output device for the test sound: {err}"))?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Test sound cancelled.".into());
    }
    stream
        .play()
        .map_err(|err| format!("Couldn't play the test sound: {err}"))?;
    std::thread::sleep(std::time::Duration::from_millis(650));
    drop(stream);
    if let Some(message) = error.lock().unwrap_or_else(|p| p.into_inner()).take() {
        return Err(message);
    }
    if callbacks.load(Ordering::Relaxed) == 0 {
        return Err(
            "Your output device did not play the test sound. Check the device and try again."
                .into(),
        );
    }
    Ok(())
}

// Let macOS mix the default-output chime itself. Opening a second HAL AudioUnit
// beside a process tap can stall while a Bluetooth headset switches profiles.
#[cfg(target_os = "macos")]
fn wav_chime() -> Vec<u8> {
    let rate = 48_000u32;
    let frames = rate / 2;
    let bytes = frames * 2;
    let mut wav = Vec::with_capacity(44 + bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for frame in 0..frames {
        wav.extend_from_slice(
            &((sample(frame as usize, rate) * i16::MAX as f32) as i16).to_le_bytes(),
        );
    }
    wav
}

#[cfg(target_os = "macos")]
fn play_default_output(cancelled: Arc<AtomicBool>) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("aside-audio-check-{}.wav", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::write(&path, wav_chime())
            .map_err(|e| format!("Couldn't prepare the test sound: {e}"))?;
        if cancelled.load(Ordering::SeqCst) {
            return Err("Test sound cancelled.".into());
        }
        let mut child = std::process::Command::new("/usr/bin/afplay")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't play the test sound: {e}"))?;
        loop {
            if cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Test sound cancelled.".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err("macOS couldn't play the test sound. Check your output device and retry.".into())
                    }
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Couldn't finish the test sound: {e}"));
                }
            }
        }
    })();
    let _ = std::fs::remove_file(path);
    result
}

pub fn play(preferred: Option<&str>, cancelled: Arc<AtomicBool>) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Test sound cancelled.".into());
    }
    #[cfg(target_os = "macos")]
    if preferred.is_none() || preferred == super::device_util::default_output_device_id().as_deref()
    {
        return play_default_output(cancelled);
    }
    let device = super::system::resolve_device(preferred)?;
    let config = device
        .default_output_config()
        .map_err(|err| format!("Couldn't check your output device: {err}"))?;
    match config.sample_format() {
        SampleFormat::F32 => play_typed::<f32>(&device, config.into(), cancelled),
        SampleFormat::F64 => play_typed::<f64>(&device, config.into(), cancelled),
        SampleFormat::I8 => play_typed::<i8>(&device, config.into(), cancelled),
        SampleFormat::I16 => play_typed::<i16>(&device, config.into(), cancelled),
        SampleFormat::I24 => play_typed::<cpal::I24>(&device, config.into(), cancelled),
        SampleFormat::I32 => play_typed::<i32>(&device, config.into(), cancelled),
        SampleFormat::I64 => play_typed::<i64>(&device, config.into(), cancelled),
        SampleFormat::U8 => play_typed::<u8>(&device, config.into(), cancelled),
        SampleFormat::U16 => play_typed::<u16>(&device, config.into(), cancelled),
        SampleFormat::U32 => play_typed::<u32>(&device, config.into(), cancelled),
        SampleFormat::U64 => play_typed::<u64>(&device, config.into(), cancelled),
        _ => Err(
            "This output device cannot play the test sound. Choose another output device.".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn native_chime_is_a_valid_half_second_pcm_file() {
        let wav = wav_chime();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 48_000);
        assert_eq!(wav.len(), 44 + 48_000);
        assert!(wav[44..]
            .chunks_exact(2)
            .any(|b| i16::from_le_bytes([b[0], b[1]]).abs() > 1000));
        assert!(wav[44 + 44_000..].iter().all(|b| *b == 0));
    }

    #[test]
    fn chime_is_quiet_finite_and_stops_after_two_notes() {
        for rate in [16000, 44100, 48000, 96000] {
            let values: Vec<_> = (0..rate as usize)
                .map(|frame| sample(frame, rate))
                .collect();
            assert!(values
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 0.081));
            assert!(values.iter().any(|value| value.abs() > 0.07));
            assert_eq!(values[0], 0.0);
            assert!(values[(rate as f32 * 0.45) as usize..]
                .iter()
                .all(|value| *value == 0.0));
            assert!(sample((rate as f32 * 0.22) as usize, rate).abs() < 0.001);
        }
    }
}
