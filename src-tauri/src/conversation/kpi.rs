//! Separate clocks for the pipelined path.
//!
//! The owner asked to measure these as distinct steps, not one "final
//! transcript then perfect answer" number:
//! 1. audio → first accurate partial
//! 2. question semantically identifiable
//! 3. question end detected
//! 4. LLM request dispatched
//! 5. first answer token
//! 6. first useful 20–30 answer tokens
//! 7. complete answer
//!
//! Primary number: interviewer finishes speaking → first useful answer
//! visible. Target p50 < 1.0s, p95 < 1.8s. That is `first_useful_from_end_ms`.

use std::time::Instant;

use serde::Serialize;

use crate::audio::channel::Channel;

/// ~25 tokens of answer body. Counted in characters because we don't have
/// a tokenizer on this path; English interview answers average ~4–5 chars
/// per token plus spaces.
pub const USEFUL_ANSWER_CHARS: usize = 120;

#[derive(Debug, Clone)]
pub struct TurnMarks {
    pub channel: Option<Channel>,
    pub turn_started_at: Option<Instant>,
    pub first_partial_at: Option<Instant>,
    pub question_identifiable_at: Option<Instant>,
    pub question_end_at: Option<Instant>,
    pub llm_dispatched_at: Option<Instant>,
    pub first_token_at: Option<Instant>,
    pub first_useful_at: Option<Instant>,
    pub complete_at: Option<Instant>,
}

impl Default for TurnMarks {
    fn default() -> Self {
        Self {
            channel: None,
            turn_started_at: None,
            first_partial_at: None,
            question_identifiable_at: None,
            question_end_at: None,
            llm_dispatched_at: None,
            first_token_at: None,
            first_useful_at: None,
            complete_at: None,
        }
    }
}

impl TurnMarks {
    pub fn begin_turn(&mut self, channel: Channel, now: Instant) {
        *self = Self {
            channel: Some(channel),
            turn_started_at: Some(now),
            ..Self::default()
        };
    }

    pub fn mark_first_partial(&mut self, now: Instant) {
        if self.first_partial_at.is_none() {
            self.first_partial_at = Some(now);
            if self.turn_started_at.is_none() {
                self.turn_started_at = Some(now);
            }
        }
    }

    pub fn mark_identifiable(&mut self, now: Instant) {
        if self.question_identifiable_at.is_none() {
            self.question_identifiable_at = Some(now);
        }
    }

    pub fn mark_question_end(&mut self, now: Instant) {
        if self.question_end_at.is_none() {
            self.question_end_at = Some(now);
        }
    }

    pub fn mark_llm_dispatched(&mut self, now: Instant) {
        if self.llm_dispatched_at.is_none() {
            self.llm_dispatched_at = Some(now);
        }
    }
}

fn delta_ms(earlier: Instant, later: Instant) -> u64 {
    later.saturating_duration_since(earlier).as_millis() as u64
}

fn from_start(marks: &TurnMarks, at: Option<Instant>) -> Option<u64> {
    match (marks.turn_started_at, at) {
        (Some(start), Some(t)) => Some(delta_ms(start, t)),
        _ => None,
    }
}

/// Frontend-facing snapshot. Missing steps stay `None` rather than `0`
/// (zero would read as "instant").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipelineKpi {
    pub audio_to_first_partial_ms: Option<u64>,
    pub question_identifiable_ms: Option<u64>,
    pub question_end_ms: Option<u64>,
    pub llm_dispatched_ms: Option<u64>,
    pub first_answer_token_ms: Option<u64>,
    pub first_useful_ms: Option<u64>,
    pub complete_ms: Option<u64>,
    /// Primary KPI: question end → first useful answer text. The number
    /// the owner compared against ParakeetAI's ~1s feel.
    pub first_useful_from_end_ms: Option<u64>,
    pub channel: Option<String>,
}

pub fn summarize(marks: &TurnMarks) -> PipelineKpi {
    let channel = marks.channel.map(|c| match c {
        Channel::Mic => "you".to_string(),
        Channel::System => "others".to_string(),
    });
    PipelineKpi {
        audio_to_first_partial_ms: from_start(marks, marks.first_partial_at),
        question_identifiable_ms: from_start(marks, marks.question_identifiable_at),
        question_end_ms: from_start(marks, marks.question_end_at),
        llm_dispatched_ms: from_start(marks, marks.llm_dispatched_at),
        first_answer_token_ms: from_start(marks, marks.first_token_at),
        first_useful_ms: from_start(marks, marks.first_useful_at),
        complete_ms: from_start(marks, marks.complete_at),
        first_useful_from_end_ms: match (marks.question_end_at, marks.first_useful_at) {
            (Some(end), Some(useful)) => Some(delta_ms(end, useful)),
            _ => None,
        },
        channel,
    }
}

pub fn log_line(k: &PipelineKpi) -> String {
    fn ms(v: Option<u64>) -> String {
        v.map(|n| format!("{n}ms"))
            .unwrap_or_else(|| "-".to_string())
    }
    format!(
        "pipeline kpi: partial={} identifiable={} end={} llm={} token={} useful={} complete={} useful-from-end={} who={}",
        ms(k.audio_to_first_partial_ms),
        ms(k.question_identifiable_ms),
        ms(k.question_end_ms),
        ms(k.llm_dispatched_ms),
        ms(k.first_answer_token_ms),
        ms(k.first_useful_ms),
        ms(k.complete_ms),
        ms(k.first_useful_from_end_ms),
        k.channel.clone().unwrap_or_else(|| "-".to_string()),
    )
}

pub const EVENT_PIPELINE_KPI: &str = "pipeline-kpi";

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_complete_turn_reports_every_step_and_the_primary_kpi() {
        let base = Instant::now();
        let marks = TurnMarks {
            channel: Some(Channel::System),
            turn_started_at: Some(base),
            first_partial_at: Some(at(base, 80)),
            question_identifiable_at: Some(at(base, 900)),
            question_end_at: Some(at(base, 2_000)),
            llm_dispatched_at: Some(at(base, 2_050)),
            first_token_at: Some(at(base, 2_400)),
            first_useful_at: Some(at(base, 2_650)),
            complete_at: Some(at(base, 4_000)),
        };
        let k = summarize(&marks);
        assert_eq!(k.audio_to_first_partial_ms, Some(80));
        assert_eq!(k.question_identifiable_ms, Some(900));
        assert_eq!(
            k.llm_dispatched_ms,
            Some(2_050),
            "Send is after question-end, never on a partial"
        );
        assert_eq!(k.first_useful_ms, Some(2_650));
        assert_eq!(
            k.first_useful_from_end_ms,
            Some(650),
            "the number compared to the 1s target"
        );
        let json = serde_json::to_string(&k).unwrap();
        assert!(json.contains("first_useful_ms"), "got {json}");
        assert!(json.contains("first_useful_from_end_ms"), "got {json}");
        assert_eq!(k.channel.as_deref(), Some("others"));
    }

    #[test]
    fn a_missing_step_is_none_not_zero() {
        let k = summarize(&TurnMarks::default());
        assert_eq!(k.first_useful_from_end_ms, None);
        assert_eq!(k.audio_to_first_partial_ms, None);
        let line = log_line(&k);
        assert!(line.contains("useful-from-end=-"), "got {line}");
    }

    #[test]
    fn useful_char_budget_is_in_the_20_to_30_token_band() {
        assert!(USEFUL_ANSWER_CHARS >= 80);
        assert!(USEFUL_ANSWER_CHARS <= 160);
    }

    #[test]
    fn first_useful_from_end_ms_is_defined_as_question_end_to_useful_not_from_start() {
        let base = Instant::now();
        let marks = TurnMarks {
            channel: Some(Channel::System),
            turn_started_at: Some(base),
            first_partial_at: Some(at(base, 50)),
            question_identifiable_at: Some(at(base, 400)),
            question_end_at: Some(at(base, 1_000)),
            llm_dispatched_at: Some(at(base, 1_020)),
            first_token_at: Some(at(base, 1_200)),
            first_useful_at: Some(at(base, 1_350)),
            complete_at: Some(at(base, 2_000)),
        };
        let k = summarize(&marks);
        assert_eq!(k.first_useful_from_end_ms, Some(350));
        assert_ne!(
            k.first_useful_from_end_ms, k.first_useful_ms,
            "from-end is not the same clock as from-start"
        );
        let json = serde_json::to_string(&k).unwrap();
        assert!(
            json.contains("\"first_useful_from_end_ms\":350"),
            "got {json}"
        );
    }
}
