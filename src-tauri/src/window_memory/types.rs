//! Conversation-window types. Pure data — no Tauri.

use serde::{Deserialize, Serialize};

use crate::audio::channel::Channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    Me,
    Other,
}

impl Speaker {
    pub fn from_channel(channel: Channel) -> Self {
        match channel {
            Channel::Mic => Speaker::Me,
            Channel::System => Speaker::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Speaker::Me => "Me",
            Speaker::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawTurn {
    pub speaker: Speaker,
    pub text: String,
    pub at_ms: u64,
}

impl RawTurn {
    pub fn line(&self) -> String {
        format!("{}: {}", self.speaker.label(), self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationWindow {
    pub turns: Vec<RawTurn>,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl ConversationWindow {
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    pub fn render(&self) -> String {
        self.turns
            .iter()
            .map(RawTurn::line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Single-layer conversation memory (owner spec, 2026-08-29 simplification —
/// replaces the earlier two-layer "recent" + re-consolidated "historical"
/// design). `summary` holds one entry per completed conversation window, in
/// order, oldest first — a 1-2 sentence prose summary written by the async
/// background compressor (`window_memory::background`), never a structured
/// fact list, and never anything the main answer model itself produces.
/// Nothing is ever re-condensed or dropped as the session grows; when the
/// assembled prompt gets too large, `prompt::build_prompt`'s own budget
/// ladder trims the oldest entries first — that's the only place `summary`
/// ever shrinks. `facts` is a separate, much smaller field for DB-backed
/// facts (visa status, notice period, etc. — `merge::merge_db_facts`): a
/// fixed key/value set, not a window summary, so it doesn't belong mixed
/// into `summary`'s chronological list. See `window_memory::background`'s
/// module doc comment for why a compressor that only ever sees the raw
/// transcript (never the visible answer) is the mechanism that keeps an
/// unspoken AI suggestion from ever being remembered as something a real
/// participant said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionMemory {
    pub facts: Vec<String>,
    pub summary: Vec<String>,
}

impl SessionMemory {
    pub fn render(&self) -> String {
        let mut out = String::from("KNOWN FACTS\n");
        if self.facts.is_empty() {
            out.push_str("- (none yet)\n");
        } else {
            for line in &self.facts {
                out.push_str("- ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str("\nCONVERSATION SO FAR (oldest first, one line per completed exchange)\n");
        if self.summary.is_empty() {
            out.push_str("- (none yet)\n");
        } else {
            for line in &self.summary {
                out.push_str("- ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }
}

/// One Command+Enter press's diagnostics. `memory_snapshot` is what memory
/// looked like at the moment this press's answer finished — **not** "before
/// vs. after this press", since this press's own conversation window is
/// summarized by a separate, asynchronous background call
/// (`window_memory::background`) that may still be in flight when this log
/// is written; the next `window-memory` event (fired when that background
/// work actually lands) is what carries the updated memory, not this log
/// entry.
#[derive(Debug, Clone, Serialize)]
pub struct PressLog {
    pub press_number: u32,
    pub window_start_ms: u64,
    pub window_end_ms: u64,
    pub raw_window: String,
    pub memory_snapshot: String,
    pub input_token_estimate: usize,
    pub output_token_estimate: usize,
    pub generated_answer: String,
    pub ttft_ms: Option<u64>,
    pub total_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowPreview {
    pub raw_window: String,
    pub facts: Vec<String>,
    pub summary: Vec<String>,
    pub press_count: u32,
    pub last_log: Option<PressLog>,
}

pub const EMPTY_WINDOW_NOTICE: &str = "Nothing to send yet — talk first, then press Command+Enter.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::channel::Channel;

    #[test]
    fn mic_is_me_and_system_is_other() {
        assert_eq!(Speaker::from_channel(Channel::Mic), Speaker::Me);
        assert_eq!(Speaker::from_channel(Channel::System), Speaker::Other);
        assert_eq!(Speaker::Me.label(), "Me");
        assert_eq!(Speaker::Other.label(), "Other");
    }

    #[test]
    fn an_empty_window_renders_blank() {
        let w = ConversationWindow {
            turns: vec![],
            start_ms: 0,
            end_ms: 1,
        };
        assert!(w.is_empty());
        assert_eq!(w.render(), "");
    }
}
