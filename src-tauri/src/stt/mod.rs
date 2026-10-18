//! Speech-to-text: turning captured audio into text.
//!
//! One engine is active at a time (`status.md` Phase 4) — only the active
//! engine's resources are ever constructed, so switching engines is also how
//! memory/network connections get freed. `SttEngine` keeps audio buffering
//! internal: callers only ever push samples and ask for what's ready — the
//! engine decides internally whether that means "nothing yet" (still
//! buffering) or "finalize now" (its own stop-of-speech fired).
//!
//! **What's implemented:** the trait, a local-only hallucination filter
//! (Parakeet EOU finals; Speechmatics already filters its own output), a
//! live Speechmatics WebSocket engine, and a local Parakeet Realtime EOU 120M
//! engine (`parakeet-rs`, one English ONNX model, downloaded and cached on
//! demand). These are the only two engines.

pub mod hallucination_filter;
pub mod parakeet_cache;
pub mod parakeet_local;
pub mod resample;
pub mod speechmatics;
pub mod speechmatics_live;
pub mod wav;
pub mod vocabulary;
pub mod connection;
mod speechmatics_buffer;

use serde::{Deserialize, Serialize};

/// Which STT engine is selected. Mirrors `status.md`'s Phase 4 scope — only
/// variants with a real `SttEngine` implementation are listed; adding one
/// here without the implementation to match would let a user select an
/// engine that silently does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// Local inference — needs the downloaded Parakeet Realtime EOU 120M
    /// model but no API key.
    ParakeetLocal,
    /// The engine ParakeetAI itself uses (named directly in their privacy
    /// policy), which is why it is here: matching that product's transcript
    /// quality is the acceptance criterion this project is measured against.
    Speechmatics,
}

impl EngineKind {
    /// The two engines Aside supports, in the order Settings lists them.
    pub const ALL: [EngineKind; 2] = [EngineKind::ParakeetLocal, EngineKind::Speechmatics];

    /// The `storage::settings` key this engine's API key is saved under, or
    /// `None` for engines that don't need one. Centralized here so the
    /// settings-persistence commands and the engine constructors can't
    /// independently drift on the key name.
    pub fn api_key_setting(self) -> Option<&'static str> {
        match self {
            EngineKind::ParakeetLocal => None,
            EngineKind::Speechmatics => Some("stt_api_key_speechmatics"),
        }
    }

    /// The stable string stored in the `settings` table for the selected
    /// engine — separate from `serde`'s wire format so a future rename of
    /// the Tauri-facing JSON casing can't silently change what's on disk.
    pub fn as_setting_value(self) -> &'static str {
        match self {
            EngineKind::ParakeetLocal => "parakeet_local",
            EngineKind::Speechmatics => "speechmatics",
        }
    }

    pub fn from_setting_value(s: &str) -> Option<Self> {
        match s {
            "parakeet_local" => Some(EngineKind::ParakeetLocal),
            "speechmatics" => Some(EngineKind::Speechmatics),
            _ => None,
        }
    }

    /// The `origin` column value `storage/transcripts.rs` writes for a
    /// segment produced by this engine — matching `storage/migrations.rs`'s
    /// `transcript_segments` origin `CHECK` constraint exactly. Deliberately a separate mapping from `as_setting_value`
    /// above: that string (`"parakeet_local"`) predates this column and
    /// doesn't match the schema's spelling (`"local_parakeet"`). Centralizing
    /// it here, next to the setting-value mapping it's easy to confuse this
    /// with, is what keeps `pipeline.rs` from having to invent (and
    /// potentially drift on) its own copy.
    pub fn origin_db_value(self) -> &'static str {
        match self {
            EngineKind::ParakeetLocal => "local_parakeet",
            EngineKind::Speechmatics => "speechmatics",
        }
    }
}

/// Rejects an empty/whitespace-only API key before it's saved.
///
/// Without this, `commands::stt::has_stt_api_key` (which only checks
/// `.is_some()`) would report an engine as "configured" for a key that can
/// never actually authenticate — a settings UI built on that would show a
/// green checkmark for something guaranteed to fail on first use.
pub fn validate_api_key(key: &str) -> Result<(), String> {
    normalize_api_key(key).map(|_| ())
}

/// Paste from a portal often includes a trailing newline, wrapping quotes, or
/// a copied `Bearer ` prefix. Store and send the bare key, otherwise
/// Speechmatics treats it as a different (invalid) credential.
pub fn normalize_api_key(key: &str) -> Result<String, String> {
    let mut trimmed = key.trim().trim_start_matches('\u{feff}').trim().to_string();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let quote = bytes[0];
        if (quote == b'"' || quote == b'\'') && bytes[trimmed.len() - 1] == quote {
            trimmed = trimmed[1..trimmed.len() - 1].trim().to_string();
        }
    }
    if let Some(rest) = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .or_else(|| trimmed.strip_prefix("BEARER "))
    {
        trimmed = rest.trim().to_string();
    }
    if trimmed.is_empty() {
        Err("API key cannot be empty".to_string())
    } else {
        Ok(trimmed)
    }
}

/// One piece of transcribed text ready to show or store.
///
/// **Contract every `SttEngine` implementation follows:**
///
/// - `is_final: false` — `text` is the current preview. A consumer
///   **replaces** its live preview with it and never appends. Engines differ
///   in what that preview spans (Parakeet restates the whole open utterance;
///   Speechmatics' partials cover only the not-yet-finalized tail), and that
///   is fine: "replace the preview" renders identically either way.
/// - `is_final: true` — `text` is a **complete, self-contained** piece of
///   transcript. A consumer appends it to the permanent log and clears the
///   preview.
///
/// The word *complete* in that second rule is load-bearing and was learned
/// the hard way. An earlier local-engine draft emitted only the part of the
/// final that partials had not already displayed, on the assumption that
/// consumers would join the two. **They do not** — `SttBench.tsx` and
/// `PanelWindow.tsx` both clear the preview and log the final's text alone
/// — so most of every long sentence was silently destroyed. See
/// `Stabilizer::commit_final`'s doc comment for the full story. Any new
/// engine must emit whole finals.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    pub text: String,
    pub is_final: bool,
}

/// A pluggable speech-to-text engine. Implementors own their audio buffering
/// internally — `push_audio` never blocks on a network call, and `drain`
/// never blocks waiting for one either; it returns whatever's ready right
/// now. `flush` is the one call allowed to do extra work (e.g. a local engine
/// padding silence so a trailing EOU can fire), because it's only called
/// when transcription stops — not on every speaking-light pause.
#[async_trait::async_trait]
pub trait SttEngine: Send {
    fn connection_status(&self) -> Option<tokio::sync::watch::Receiver<connection::SpeechConnection>> { None }
    /// Buffers audio for later transcription. `samples` are mono `f32` in
    /// `[-1.0, 1.0]`.
    fn push_audio(&mut self, samples: &[f32], sample_rate: u32);

    /// Returns any transcripts that became available on their own since the
    /// last call (relevant for streaming engines; a local engine that only
    /// produces results on `flush` returns empty here when the buffer is
    /// still too short to be worth a preview pass).
    async fn drain(&mut self) -> Result<Vec<Transcript>, String>;

    /// Signals the engine should commit whatever it still has open. Called
    /// when transcription **stops**, not on speaking-light VAD — every live
    /// engine already has its own endpointing.
    async fn flush(&mut self) -> Result<Vec<Transcript>, String>;

    /// Releases any held resources (open sockets, buffers). Takes `self` by
    /// value via `Box` so it can be called on a `Box<dyn SttEngine>`.
    async fn shutdown(self: Box<Self>) -> Vec<Transcript>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_kind_round_trips_through_its_setting_value() {
        for kind in EngineKind::ALL {
            let value = kind.as_setting_value();
            assert_eq!(EngineKind::from_setting_value(value), Some(kind));
        }
    }

    #[test]
    fn an_unrecognized_setting_value_is_none_not_a_panic() {
        assert_eq!(EngineKind::from_setting_value("bogus_engine"), None);
        assert_eq!(EngineKind::from_setting_value(""), None);
    }

    #[test]
    fn api_key_settings_are_distinct_across_the_engines_that_have_one() {
        let mut seen = std::collections::HashSet::new();
        for kind in EngineKind::ALL {
            if let Some(key) = kind.api_key_setting() {
                assert!(seen.insert(key), "duplicate api_key_setting for {kind:?}");
            }
        }
    }

    #[test]
    fn parakeet_local_needs_no_api_key() {
        assert_eq!(EngineKind::ParakeetLocal.api_key_setting(), None);
    }

    #[test]
    fn origin_db_value_matches_the_transcript_segments_check_constraint_spelling() {
        // storage/migrations.rs's CHECK constraint pins these exact three
        // strings; this test fails loudly if either list is ever edited
        // without the other.
        let expected = ["local_parakeet", "speechmatics"];
        for (kind, want) in EngineKind::ALL.iter().zip(expected) {
            assert_eq!(
                kind.origin_db_value(),
                want,
                "{kind:?}'s origin_db_value drifted from the schema"
            );
        }
    }

    #[test]
    fn origin_db_value_and_as_setting_value_are_deliberately_different_for_parakeet_local() {
        // Regression guard for the exact mix-up this mapping exists to
        // prevent: as_setting_value is "parakeet_local" (the settings-table
        // spelling), origin_db_value is "local_parakeet" (the schema's
        // spelling) — easy to transpose, so pin both here.
        assert_eq!(
            EngineKind::ParakeetLocal.as_setting_value(),
            "parakeet_local"
        );
        assert_eq!(
            EngineKind::ParakeetLocal.origin_db_value(),
            "local_parakeet"
        );
    }

    #[test]
    fn every_live_engine_has_native_endpointing() {
        // The filter matrix: VAD must not flush STT because each of these
        // already detects "they stopped talking" (EOU token / EndOfUtterance). Adding an engine without native endpointing means
        // this test must be revisited, not silently inheriting the skip.
        for kind in EngineKind::ALL {
            match kind {
                EngineKind::ParakeetLocal | EngineKind::Speechmatics => {}
            }
        }
        assert_eq!(EngineKind::ALL.len(), 2);
        assert_eq!(EngineKind::from_setting_value("deepgram"), None, "removed engine");
    }

    #[test]
    fn empty_and_whitespace_only_keys_are_rejected() {
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("   ").is_err());
        assert!(validate_api_key("\t\n").is_err());
    }

    #[test]
    fn a_real_looking_key_is_accepted() {
        assert!(validate_api_key("sm_abc123").is_ok());
        assert_eq!(normalize_api_key("  abc123 \n").unwrap(), "abc123");
        assert_eq!(normalize_api_key("Bearer abc123").unwrap(), "abc123");
        assert_eq!(normalize_api_key("\"abc123\"").unwrap(), "abc123");
        assert_eq!(normalize_api_key("\u{feff}abc123").unwrap(), "abc123");
    }
}
