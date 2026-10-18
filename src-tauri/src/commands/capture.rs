//! Live capture/transcription status for the always-visible status strip.
//!
//! This module used to also hold the old product's "Connect" session-start
//! orchestration (`check_session_readiness`, `start_session_capture`,
//! `retry_channel_capture`, `get_last_capture_report`) — removed 2026-08-27
//! along with the dead `SessionSetup.tsx`/`PanelWindow.tsx` windows that were
//! its only callers (see CURRENT.md). [`get_capture_status`] survives because
//! `src/diagnostics/useAudioChannel.ts` still polls it.

use serde::Serialize;
use tauri::State;

use crate::audio::channel::Channel;
use crate::commands::audio::{self, AudioState, SystemAudioState};
use crate::pipeline::{self, TranscriptionRouting};

/// Live capture/transcription state per channel, for the always-visible
/// status strip. Read-only and cheap — safe to poll or to re-request after
/// any `capture-status` event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaptureStatus {
    pub mic_capturing: bool,
    pub mic_transcribing: bool,
    pub system_capturing: bool,
    pub system_transcribing: bool,
}

#[tauri::command(async)]
pub fn get_capture_status(
    routing: State<TranscriptionRouting>,
    audio_state: State<AudioState>,
    system_state: State<SystemAudioState>,
) -> CaptureStatus {
    CaptureStatus {
        mic_capturing: audio::is_mic_capturing(&audio_state),
        mic_transcribing: pipeline::is_running(&routing, Channel::Mic),
        system_capturing: audio::is_system_capturing(&system_state),
        system_transcribing: pipeline::is_running(&routing, Channel::System),
    }
}
