//! Sample-level echo cancellation for the **Me** ear, laptop speakers only.
//!
//! Laptop speakers play the call into the room. The open mic hears that copy
//! as well as the owner's real voice. This module tells a WebRTC AEC3 engine
//! what just played (system tap, unchanged for Others STT) and asks it to
//! strip that copy from the mic before Me STT.
//!
//! Headphones / AirPods leave this off — those two pipes are already separate.
//! Work runs on a worker thread, not inside the cpal callback. We never use
//! Apple Voice Processing IO (it fights the tap and can wreck AirPods).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

use super::channel::{StreamMsg, SttAudioFilter};
use super::device_util::is_open_loudspeaker;
use crate::stt::resample::StreamingResampler;

const AEC_SAMPLE_RATE: u32 = 16_000;
const FRAME_LEN: usize = 160; // 10 ms at 16 kHz
const QUEUE_CAP: usize = 64;
const STREAM_DELAY_MS: i32 = 50;
const RENDER_CAP: usize = AEC_SAMPLE_RATE as usize;

enum AecMsg {
    Render {
        samples: Vec<f32>,
        sample_rate: u32,
    },
    Capture {
        samples: Vec<f32>,
        sample_rate: u32,
        stt: std::sync::mpsc::Sender<StreamMsg>,
    },
}

struct AecInner {
    enabled: AtomicBool,
    tx: SyncSender<AecMsg>,
}

/// Shared handle: cpal callbacks only `try_send`; the worker runs AEC3.
#[derive(Clone)]
pub struct AecHub(Arc<AecInner>);

impl AecHub {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAP);
        let _ = std::thread::Builder::new()
            .name("aside-aec".into())
            .spawn(move || worker(rx));
        Self(Arc::new(AecInner {
            enabled: AtomicBool::new(false),
            tx,
        }))
    }

    pub fn set_enabled(&self, on: bool) {
        self.0.enabled.store(on, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.0.enabled.load(Ordering::SeqCst)
    }

    /// Turns AEC on only for open laptop speakers. Returns the new state.
    pub fn set_enabled_for_output(&self, id: Option<&str>, name: Option<&str>) -> bool {
        let on = should_enable(id, name);
        self.set_enabled(on);
        on
    }

    fn try_capture(
        &self,
        samples: &[f32],
        sample_rate: u32,
        stt: &std::sync::mpsc::Sender<StreamMsg>,
    ) -> bool {
        if !self.is_enabled() {
            return false;
        }
        match self.0.tx.try_send(AecMsg::Capture {
            samples: samples.to_vec(),
            sample_rate,
            stt: stt.clone(),
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn try_render(&self, samples: &[f32], sample_rate: u32) {
        if !self.is_enabled() {
            return;
        }
        let _ = self.0.tx.try_send(AecMsg::Render {
            samples: samples.to_vec(),
            sample_rate,
        });
    }
}

impl Default for AecHub {
    fn default() -> Self {
        Self::new()
    }
}

impl SttAudioFilter for AecHub {
    fn intercept_capture(
        &self,
        samples: &[f32],
        sample_rate: u32,
        tx: &std::sync::mpsc::Sender<StreamMsg>,
    ) -> bool {
        self.try_capture(samples, sample_rate, tx)
    }

    fn tap_render(&self, samples: &[f32], sample_rate: u32) {
        self.try_render(samples, sample_rate);
    }
}

/// True only for the laptop's open speakers — never AirPods / headphones.
pub fn should_enable(output_id: Option<&str>, output_name: Option<&str>) -> bool {
    is_open_loudspeaker(output_id, output_name)
}

fn make_apm() -> AudioProcessing {
    let cfg = StreamConfig::new(AEC_SAMPLE_RATE, 1);
    let mut apm = AudioProcessing::builder()
        .config(Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        })
        .capture_config(cfg)
        .render_config(cfg)
        .build();
    let _ = apm.set_stream_delay_ms(STREAM_DELAY_MS);
    apm
}

fn worker(rx: Receiver<AecMsg>) {
    let mut apm = make_apm();
    let mut render_buf = Vec::new();
    let mut capture_buf = Vec::new();
    let mut render_dest = vec![0.0f32; FRAME_LEN];
    let mut capture_out = vec![0.0f32; FRAME_LEN];
    let mut render_resampler = StreamingResampler::default();
    let mut capture_resampler = StreamingResampler::default();
    while let Ok(msg) = rx.recv() {
        match msg {
            AecMsg::Render {
                samples,
                sample_rate,
            } => {
                let resampled = render_resampler.process(&samples, sample_rate, AEC_SAMPLE_RATE);
                render_buf.extend_from_slice(&resampled);
                if render_buf.len() > RENDER_CAP {
                    let extra = render_buf.len() - RENDER_CAP;
                    render_buf.drain(..extra);
                }
                drain_render(&mut apm, &mut render_buf, &mut render_dest);
            }
            AecMsg::Capture {
                samples,
                sample_rate,
                stt,
            } => {
                drain_render(&mut apm, &mut render_buf, &mut render_dest);
                let resampled = capture_resampler.process(&samples, sample_rate, AEC_SAMPLE_RATE);
                capture_buf.extend_from_slice(&resampled);
                while capture_buf.len() >= FRAME_LEN {
                    let frame: Vec<f32> = capture_buf.drain(..FRAME_LEN).collect();
                    capture_out.fill(0.0);
                    let cleaned = if apm
                        .process_capture_f32(&[&frame], &mut [&mut capture_out[..]])
                        .is_ok()
                    {
                        capture_out.clone()
                    } else {
                        frame
                    };
                    let _ = stt.send(StreamMsg::Audio {
                        samples: cleaned,
                        sample_rate: AEC_SAMPLE_RATE,
                    });
                }
            }
        }
    }
}

fn drain_render(apm: &mut AudioProcessing, buf: &mut Vec<f32>, dest: &mut [f32]) {
    while buf.len() >= FRAME_LEN {
        let frame: Vec<f32> = buf.drain(..FRAME_LEN).collect();
        dest.fill(0.0);
        let _ = apm.process_render_f32(&[&frame], &mut [&mut dest[..]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aec_is_on_for_built_in_speakers_and_off_for_airpods() {
        assert!(should_enable(
            Some("BuiltInSpeakerDevice:output"),
            Some("MacBook Pro Speakers")
        ));
        assert!(should_enable(Some("BuiltInSpeakerDevice"), None));
        assert!(!should_enable(
            Some("38-C4-3A-5A-28-F0:output"),
            Some("Devendra's AirPods Pro")
        ));
        assert!(!should_enable(
            Some("BuiltInMicrophoneDevice"),
            Some("MacBook Pro Microphone")
        ));
        assert!(!should_enable(None, Some("External Headphones")));
    }

    #[test]
    fn disabled_hub_does_not_intercept_mic_audio() {
        let hub = AecHub::new();
        hub.set_enabled(false);
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(
            !hub.intercept_capture(&[0.1, 0.2, 0.3], 16_000, &tx),
            "AirPods / AEC-off must leave Me STT on the original samples"
        );
        assert!(
            rx.try_recv().is_err(),
            "disabled AEC must not consume the buffer"
        );
    }

    #[test]
    fn enabled_hub_queues_capture_off_the_caller_thread() {
        let hub = AecHub::new();
        hub.set_enabled(true);
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(
            hub.intercept_capture(&[0.0f32; 160], 16_000, &tx),
            "loudspeaker AEC must take the mic buffer onto the worker"
        );
        let got = rx.recv_timeout(std::time::Duration::from_secs(2));
        assert!(got.is_ok(), "worker must still forward audio to Me STT");
        match got.unwrap() {
            StreamMsg::Audio {
                samples,
                sample_rate,
            } => {
                assert_eq!(sample_rate, AEC_SAMPLE_RATE);
                assert_eq!(samples.len(), FRAME_LEN);
            }
            other => panic!("expected Audio, got {other:?}"),
        }
    }

    #[test]
    fn sonora_aec3_modifies_a_capture_frame_that_contains_render() {
        // Same shape as sonora's own simple example: render leaks into capture.
        let mut apm = make_apm();
        let mut render = vec![0.0f32; FRAME_LEN];
        let mut capture = vec![0.0f32; FRAME_LEN];
        for i in 0..FRAME_LEN {
            let s = (i as f32 * 0.15).sin() * 0.4;
            render[i] = s;
            capture[i] = s * 0.7;
        }
        let mut render_out = vec![0.0f32; FRAME_LEN];
        apm.process_render_f32(&[&render], &mut [&mut render_out[..]])
            .expect("render frame");
        let mut capture_out = vec![0.0f32; FRAME_LEN];
        apm.process_capture_f32(&[&capture], &mut [&mut capture_out[..]])
            .expect("capture frame");
        assert_ne!(
            capture, capture_out,
            "AEC3 must change a capture frame that contains the render signal"
        );
    }
}
