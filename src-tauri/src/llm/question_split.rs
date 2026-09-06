//! Pure, chunk-boundary-agnostic parser for the `Q: <cleaned question>\nA:
//! <answer>` shape `prompt::SYSTEM_PROMPT` asks every provider to stream
//! back (2026-08-25, owner request — see that constant's doc comment for
//! why this is one delimited stream instead of a second network call).
//!
//! Deliberately a small buffering state machine, not a regex/line-scanner —
//! SSE deltas can split "Q:"/"A:" across arbitrary byte boundaries (a
//! provider might send "A" in one chunk and ": the answer is" in the next),
//! so the only correct approach is to buffer until the marker is found
//! (or gate on a max-buffer size, so a provider that never produces the
//! format at all doesn't withhold the whole answer forever).
//!
//! No `AppHandle`/Tauri dependency here on purpose — same split this
//! project already applies everywhere else (`llm::build_context_string`,
//! `active_session::should_clear`, ...): the parsing logic is pure and
//! fully unit-testable; `llm::mod`'s `SplittingSink` is the thin wrapper
//! that turns this into real `AnswerSink` calls.

const MARKER: &str = "A:";

/// A model that ignores the `Q:`/`A:` format entirely must not withhold the
/// whole answer forever while this waits for a marker that will never
/// arrive. Comfortably longer than any real one-sentence question
/// restatement, short enough that a real answer's first visible text still
/// shows up quickly.
const MAX_BUFFER_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq)]
pub enum SplitEvent {
    /// Still buffering — nothing to show the caller yet.
    Waiting,
    /// The `A:` marker was just found (or the buffer hit its cap without
    /// one). `cleaned_question` is empty when the marker was never found —
    /// callers should fall back to displaying the raw question in that
    /// case. `answer_text` is any answer text that arrived in the same
    /// chunk as the marker (or the whole flushed buffer, in the cap case)
    /// and should be forwarded immediately, not dropped.
    Resolved {
        cleaned_question: String,
        answer_text: String,
    },
    /// Already past the marker (or the cap) — plain answer text to forward.
    Answer(String),
}

#[derive(Default)]
pub struct QuestionAnswerSplitter {
    buffer: String,
    resolved: bool,
}

impl QuestionAnswerSplitter {
    pub fn push(&mut self, chunk: &str) -> SplitEvent {
        if self.resolved {
            return SplitEvent::Answer(chunk.to_string());
        }
        self.buffer.push_str(chunk);

        if let Some(pos) = self.buffer.find(MARKER) {
            self.resolved = true;
            let cleaned_question = clean_prefix(&self.buffer[..pos]);
            let answer_text = self.buffer[pos + MARKER.len()..].trim_start().to_string();
            return SplitEvent::Resolved {
                cleaned_question,
                answer_text,
            };
        }

        if self.buffer.len() > MAX_BUFFER_CHARS {
            self.resolved = true;
            let flushed = std::mem::take(&mut self.buffer);
            return SplitEvent::Resolved {
                cleaned_question: String::new(),
                answer_text: flushed,
            };
        }

        SplitEvent::Waiting
    }

    /// Called once the provider's stream ends. Flushes anything still
    /// buffered (the marker never arrived at all — e.g. a very short reply,
    /// or a provider/fallback message that never followed the format) as
    /// plain answer text instead of silently losing it. `None` if there was
    /// nothing left to flush.
    pub fn finish(&mut self) -> Option<String> {
        if self.resolved || self.buffer.is_empty() {
            return None;
        }
        self.resolved = true;
        Some(std::mem::take(&mut self.buffer))
    }
}

/// Strips a leading `Q:` label (and surrounding whitespace/newlines) from
/// the text buffered before the `A:` marker. Tolerant of the model omitting
/// the `Q:` label outright — the restated question text is still usable.
fn clean_prefix(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix("Q:")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_chunk_containing_the_full_shape_splits_cleanly() {
        let mut s = QuestionAnswerSplitter::default();
        let event = s.push("Q: What is your biggest weakness?\nA: I sometimes over-prepare.");
        assert_eq!(
            event,
            SplitEvent::Resolved {
                cleaned_question: "What is your biggest weakness?".to_string(),
                answer_text: "I sometimes over-prepare.".to_string(),
            }
        );
    }

    #[test]
    fn the_marker_split_across_chunk_boundaries_is_still_found() {
        let mut s = QuestionAnswerSplitter::default();
        assert_eq!(s.push("Q: What is your"), SplitEvent::Waiting);
        assert_eq!(s.push(" biggest weakness?\n"), SplitEvent::Waiting);
        // "A:" itself split across two pushes.
        assert_eq!(s.push("A"), SplitEvent::Waiting);
        let event = s.push(": I sometimes over-prepare.");
        assert_eq!(
            event,
            SplitEvent::Resolved {
                cleaned_question: "What is your biggest weakness?".to_string(),
                answer_text: "I sometimes over-prepare.".to_string(),
            }
        );
    }

    #[test]
    fn chunks_after_the_marker_forward_as_plain_answer_text() {
        let mut s = QuestionAnswerSplitter::default();
        s.push("Q: q\nA: first ");
        assert_eq!(s.push("second "), SplitEvent::Answer("second ".to_string()));
        assert_eq!(s.push("third"), SplitEvent::Answer("third".to_string()));
    }

    #[test]
    fn a_provider_that_never_produces_the_marker_flushes_at_the_buffer_cap() {
        let mut s = QuestionAnswerSplitter::default();
        let long_text = "x".repeat(MAX_BUFFER_CHARS + 1);
        let event = s.push(&long_text);
        assert_eq!(
            event,
            SplitEvent::Resolved {
                cleaned_question: String::new(),
                answer_text: long_text
            }
        );
        // Resolved — everything after this point is plain answer text.
        assert_eq!(s.push("more"), SplitEvent::Answer("more".to_string()));
    }

    #[test]
    fn finish_flushes_a_marker_that_never_arrived() {
        let mut s = QuestionAnswerSplitter::default();
        s.push("just a short reply, no format");
        assert_eq!(
            s.finish(),
            Some("just a short reply, no format".to_string())
        );
        // A second finish (or push) after resolution must not re-flush.
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn finish_after_the_marker_was_already_found_flushes_nothing() {
        let mut s = QuestionAnswerSplitter::default();
        s.push("Q: q\nA: already resolved");
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn finish_on_an_empty_stream_flushes_nothing() {
        let mut s = QuestionAnswerSplitter::default();
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn a_missing_q_label_still_yields_the_buffered_text_as_the_cleaned_question() {
        let mut s = QuestionAnswerSplitter::default();
        let event = s.push("what is your biggest weakness\nA: I over-prepare.");
        assert_eq!(
            event,
            SplitEvent::Resolved {
                cleaned_question: "what is your biggest weakness".to_string(),
                answer_text: "I over-prepare.".to_string(),
            }
        );
    }
}
