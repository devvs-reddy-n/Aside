//! Stream the visible answer as it arrives.
//!
//! The main model's reply is plain text only (2026-08-29 rewrite) — no
//! embedded JSON, no `---ASIDE_META---` trailer. Conversation memory moved
//! to a separate, asynchronous, cheap-model pipeline
//! (`window_memory::background`) that is fed only the raw completed
//! transcript window, never this answer text — see that module's doc
//! comment for why that split, not a parser-level filter, is what keeps an
//! unspoken AI suggestion from ever being remembered as something a real
//! participant said. This module is now just a thin pass-through, kept as
//! its own type (rather than inlining `String` everywhere) so
//! `llm::WindowAnswerSink` has one small, testable seam between the raw
//! provider stream and the visible answer.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedResponse {
    pub answer: String,
}

#[derive(Debug, Default)]
pub struct StreamParser {
    buf: String,
}

impl StreamParser {
    /// Every chunk is visible answer text — return it unchanged so it can
    /// stream straight to the UI.
    pub fn push(&mut self, chunk: &str) -> String {
        self.buf.push_str(chunk);
        chunk.to_string()
    }

    /// Nothing is ever held back waiting to see if it becomes something
    /// else, so there is never anything left to flush.
    pub fn flush(&mut self) -> String {
        String::new()
    }

    pub fn finish(&mut self) -> ParsedResponse {
        ParsedResponse {
            answer: self.buf.trim().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_streams_through_unchanged() {
        let mut p = StreamParser::default();
        let d1 = p.push("Kubernetes is");
        assert_eq!(d1, "Kubernetes is");
        let d2 = p.push(" a cluster.");
        assert_eq!(d2, " a cluster.");
        let parsed = p.finish();
        assert_eq!(parsed.answer, "Kubernetes is a cluster.");
    }

    #[test]
    fn first_character_is_visible_immediately() {
        let mut p = StreamParser::default();
        let first = p.push("Y");
        assert_eq!(
            first, "Y",
            "prose must not wait for a length threshold: {first:?}"
        );
        let rest = p.push("es.");
        assert_eq!(rest, "es.");
        let parsed = p.finish();
        assert_eq!(parsed.answer, "Yes.");
    }

    #[test]
    fn short_replies_still_reach_the_screen() {
        let mut p = StreamParser::default();
        let first = p.push("Yes.");
        assert_eq!(first, "Yes.");
        let leftover = p.flush();
        assert!(leftover.is_empty());
        let parsed = p.finish();
        assert_eq!(parsed.answer, "Yes.");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_from_the_final_answer_only() {
        let mut p = StreamParser::default();
        p.push("  Hello there.  ");
        let parsed = p.finish();
        assert_eq!(parsed.answer, "Hello there.");
    }

    #[test]
    fn an_empty_stream_finishes_as_an_empty_answer() {
        let mut p = StreamParser::default();
        let parsed = p.finish();
        assert_eq!(parsed.answer, "");
    }
}
