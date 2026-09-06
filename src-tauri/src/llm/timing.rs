//! Real measurement of how long an answer actually takes (2026-08-25).
//!
//! The owner asked for answers "under 1 sec". This module exists so that
//! claim can be checked against real milliseconds rather than asserted —
//! `CLAUDE.md`'s honest-status rule makes an unmeasured speed claim a
//! violation, not just sloppiness.
//!
//! **The budget is deliberately split into separately-named segments**, so
//! nobody can hit the easy one and imply the hard one:
//!
//! - **freeze_ms** — shortcut → frozen window. Ours.
//! - **context_ms** — frozen window → prompt assembled (typed Ask: trigger →
//!   prompt). Ours. Does not include freeze.
//! - **body_ready_ms** — trigger → JSON body assembled. Ours.
//! - **headers_ms** — request dispatched → HTTP response headers. Network.
//! - **first_sse_ms** — request dispatched → first SSE frame.
//! - **first_model_text_ms** — request dispatched → first `delta.content`.
//! - **first_visible_from_request_ms** — request dispatched → first visible
//!   answer fragment (JSON `"answer"` or prose). This is **not** literal TTFB.
//! - **ttft_ms** — trigger → first visible answer text.
//! - **total_ms** — trigger → answer finished.
//!
//! `ttfb_ms` is kept as an alias of `first_visible_from_request_ms` so
//! existing debug cards keep working. Do not read it as first network byte.
//!
//! Speech-to-text finalization delay (how long the transcription service
//! takes to decide a sentence ended) is **not** in here and is reported
//! separately by `pipeline.rs`, because it is not latency this code can fix
//! — Speechmatics' `max_delay` is already pinned at its documented floor.

use std::time::Instant;

use serde::Serialize;

/// Monotonic marks collected across one answer. `Instant`, not wall clock:
/// immune to NTP steps and clock changes mid-measurement.
#[derive(Debug, Clone)]
pub struct AnswerMarks {
    pub trigger_at: Instant,
    pub frozen_at: Option<Instant>,
    pub context_built_at: Option<Instant>,
    pub body_ready_at: Option<Instant>,
    pub request_sent_at: Option<Instant>,
    pub headers_at: Option<Instant>,
    pub first_sse_at: Option<Instant>,
    pub first_model_text_at: Option<Instant>,
    pub first_delta_at: Option<Instant>,
    pub done_at: Option<Instant>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_chars: usize,
    pub first_useful_at: Option<Instant>,
    pub answer_preview: Option<String>,
    pub answer_chars: usize,
    pub usage: TokenUsage,
}

impl AnswerMarks {
    pub fn start() -> Self {
        Self {
            trigger_at: Instant::now(),
            frozen_at: None,
            context_built_at: None,
            body_ready_at: None,
            request_sent_at: None,
            headers_at: None,
            first_sse_at: None,
            first_model_text_at: None,
            first_delta_at: None,
            done_at: None,
            provider: None,
            model: None,
            context_chars: 0,
            first_useful_at: None,
            answer_preview: None,
            answer_chars: 0,
            usage: TokenUsage::default(),
        }
    }
}

/// Token counts from the provider's final usage object, when it sends one.
/// All optional: a stream that never reports usage must not look like zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u32>,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

/// The derived, frontend-facing shape. Every field is `Option` where the
/// underlying mark can legitimately be missing — an answer that failed
/// before the provider ever replied has no `ttfb_ms`, and reporting `0`
/// there would read as "instant" instead of "never happened".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimingSummary {
    pub freeze_ms: Option<u64>,
    pub context_ms: Option<u64>,
    pub body_ready_ms: Option<u64>,
    pub headers_ms: Option<u64>,
    pub first_sse_ms: Option<u64>,
    pub first_model_text_ms: Option<u64>,
    /// Request dispatched → first visible answer text. Not literal first network byte.
    pub first_visible_from_request_ms: Option<u64>,
    /// Alias of `first_visible_from_request_ms` for existing debug cards.
    pub ttfb_ms: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub total_ms: Option<u64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_chars: usize,
    pub input_tokens: Option<u32>,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

/// Saturating millisecond delta. `later` before `earlier` yields `0` rather
/// than panicking — `Instant::duration_since` panics on some platforms for
/// a reversed pair, and a mis-ordered mark must never take down a live
/// session over a diagnostic.
fn delta_ms(earlier: Instant, later: Instant) -> u64 {
    later.saturating_duration_since(earlier).as_millis() as u64
}

pub fn summarize(marks: &AnswerMarks) -> TimingSummary {
    let first_visible_from_request_ms = match (marks.request_sent_at, marks.first_delta_at) {
        (Some(sent), Some(first)) => Some(delta_ms(sent, first)),
        _ => None,
    };
    let context_ms = match (marks.frozen_at, marks.context_built_at) {
        (Some(frozen), Some(ready)) => Some(delta_ms(frozen, ready)),
        (None, Some(ready)) => Some(delta_ms(marks.trigger_at, ready)),
        _ => None,
    };
    TimingSummary {
        freeze_ms: marks.frozen_at.map(|t| delta_ms(marks.trigger_at, t)),
        context_ms,
        body_ready_ms: marks.body_ready_at.map(|t| delta_ms(marks.trigger_at, t)),
        headers_ms: match (marks.request_sent_at, marks.headers_at) {
            (Some(sent), Some(headers)) => Some(delta_ms(sent, headers)),
            _ => None,
        },
        first_sse_ms: match (marks.request_sent_at, marks.first_sse_at) {
            (Some(sent), Some(sse)) => Some(delta_ms(sent, sse)),
            _ => None,
        },
        first_model_text_ms: match (marks.request_sent_at, marks.first_model_text_at) {
            (Some(sent), Some(text)) => Some(delta_ms(sent, text)),
            _ => None,
        },
        first_visible_from_request_ms,
        ttfb_ms: first_visible_from_request_ms,
        ttft_ms: marks.first_delta_at.map(|t| delta_ms(marks.trigger_at, t)),
        total_ms: marks.done_at.map(|t| delta_ms(marks.trigger_at, t)),
        provider: marks.provider.clone(),
        model: marks.model.clone(),
        context_chars: marks.context_chars,
        input_tokens: marks.usage.input_tokens,
        cached_input_tokens: marks.usage.cached_input_tokens,
        reasoning_tokens: marks.usage.reasoning_tokens,
        output_tokens: marks.usage.output_tokens,
    }
}

/// One-line terminal readout, so `cargo run` alone is enough to see the
/// numbers without the bench window open.
pub fn log_line(s: &TimingSummary) -> String {
    fn ms(v: Option<u64>) -> String {
        v.map(|n| format!("{n}ms"))
            .unwrap_or_else(|| "-".to_string())
    }
    format!(
        "answer timing: freeze={} context={} body={} headers={} first-sse={} first-model={} first-visible={} first-word={} total={} model={} context={}chars in={} cached={} reason={} out={}",
        ms(s.freeze_ms),
        ms(s.context_ms),
        ms(s.body_ready_ms),
        ms(s.headers_ms),
        ms(s.first_sse_ms),
        ms(s.first_model_text_ms),
        ms(s.first_visible_from_request_ms),
        ms(s.ttft_ms),
        ms(s.total_ms),
        s.model.clone().unwrap_or_else(|| "-".to_string()),
        s.context_chars,
        s.input_tokens.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
        s.cached_input_tokens.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
        s.reasoning_tokens.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
        s.output_tokens.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
    )
}

pub const EVENT_ANSWER_TIMING: &str = "answer-timing";

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn marks_at(base: Instant) -> AnswerMarks {
        AnswerMarks {
            trigger_at: base,
            frozen_at: Some(at(base, 4)),
            context_built_at: Some(at(base, 12)),
            body_ready_at: Some(at(base, 18)),
            request_sent_at: Some(at(base, 45)),
            headers_at: Some(at(base, 120)),
            first_sse_at: Some(at(base, 200)),
            first_model_text_at: Some(at(base, 650)),
            first_delta_at: Some(at(base, 700)),
            done_at: Some(at(base, 2_100)),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            context_chars: 1234,
            first_useful_at: Some(at(base, 900)),
            answer_preview: Some("opening bullets".to_string()),
            answer_chars: 120,
            usage: TokenUsage {
                input_tokens: Some(800),
                cached_input_tokens: Some(400),
                reasoning_tokens: Some(0),
                output_tokens: Some(60),
            },
        }
    }

    #[test]
    fn a_complete_run_computes_every_segment() {
        let base = Instant::now();
        let s = summarize(&marks_at(base));
        assert_eq!(s.freeze_ms, Some(4));
        assert_eq!(
            s.context_ms,
            Some(8),
            "context_ms is freeze → prompt, not trigger → prompt"
        );
        assert_eq!(s.body_ready_ms, Some(18));
        assert_eq!(s.headers_ms, Some(75));
        assert_eq!(s.first_sse_ms, Some(155));
        assert_eq!(s.first_model_text_ms, Some(605));
        assert_eq!(s.first_visible_from_request_ms, Some(655));
        assert_eq!(
            s.ttfb_ms,
            Some(655),
            "alias of first visible text from request, not first network byte"
        );
        assert_eq!(s.ttft_ms, Some(700));
        assert_eq!(s.total_ms, Some(2_100));
        assert_eq!(s.context_chars, 1234);
        assert_eq!(s.input_tokens, Some(800));
        assert_eq!(s.cached_input_tokens, Some(400));
        assert_eq!(s.output_tokens, Some(60));
    }

    #[test]
    fn a_request_that_never_got_a_reply_reports_nothing_rather_than_zero() {
        // The distinction that matters: `Some(0)` reads as "instant",
        // `None` reads as "never happened". A failed provider call is the
        // second thing, and must not flatter the numbers.
        let base = Instant::now();
        let mut m = marks_at(base);
        m.first_delta_at = None;
        m.done_at = Some(at(base, 5_000));
        let s = summarize(&m);
        assert_eq!(s.ttfb_ms, None);
        assert_eq!(s.ttft_ms, None);
        assert_eq!(s.total_ms, Some(5_000));
    }

    #[test]
    fn a_run_that_never_finished_still_reports_what_it_has() {
        let base = Instant::now();
        let mut m = marks_at(base);
        m.done_at = None;
        let s = summarize(&m);
        assert_eq!(s.total_ms, None);
        assert_eq!(
            s.ttft_ms,
            Some(700),
            "a partial run must not lose the marks it did collect"
        );
    }

    #[test]
    fn a_bare_start_summarizes_to_all_none_without_panicking() {
        let s = summarize(&AnswerMarks::start());
        assert_eq!(s.context_ms, None);
        assert_eq!(s.ttfb_ms, None);
        assert_eq!(s.ttft_ms, None);
        assert_eq!(s.total_ms, None);
    }

    #[test]
    fn out_of_order_marks_saturate_to_zero_instead_of_panicking() {
        // Defensive: a diagnostic must never be able to crash a live
        // session. `Instant::duration_since` can panic on a reversed pair.
        let base = Instant::now();
        let later = at(base, 1_000);
        let mut m = AnswerMarks::start();
        m.trigger_at = later;
        m.first_delta_at = Some(base);
        let s = summarize(&m);
        assert_eq!(s.ttft_ms, Some(0));
    }

    #[test]
    fn the_log_line_shows_a_dash_for_a_missing_segment_not_a_zero() {
        let base = Instant::now();
        let mut m = marks_at(base);
        m.first_delta_at = None;
        let line = log_line(&summarize(&m));
        assert!(line.contains("first-word=-"), "got {line}");
        assert!(line.contains("gpt-4o-mini"));
    }
}
