//! Rolling "what is this person currently saying" buffer.
//!
//! Local Parakeet and Deepgram restate the whole open utterance on every
//! partial. Speechmatics sends finalized prefixes as deltas and only the
//! unfinalized tail as a partial. This buffer hides that difference so the
//! question detector always sees one live string.

use super::similarity::is_restatement;

#[derive(Debug, Clone, Default)]
pub struct ChannelBuffer {
    committed: Vec<String>,
    partial: String,
}

impl ChannelBuffer {
    pub fn live(&self) -> String {
        let committed = self.committed.join(" ");
        let committed = committed.trim();
        let partial = self.partial.trim();
        if committed.is_empty() {
            partial.to_string()
        } else if partial.is_empty() {
            committed.to_string()
        } else {
            format!("{committed} {partial}")
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.committed.clear();
        self.partial.clear();
    }

    /// Push one STT event. `is_final` follows `stt::Transcript`'s contract.
    pub fn push(&mut self, text: &str, is_final: bool) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        if is_final {
            let live = self.live();
            if is_restatement(&live, text) {
                // Parakeet/Deepgram: the final restates the whole open utterance.
                self.committed = vec![text.to_string()];
            } else {
                // Speechmatics: a finalized prefix or tail — append.
                self.committed.push(text.to_string());
            }
            self.partial.clear();
            return;
        }

        // Partial.
        let live = self.live();
        if is_restatement(&self.partial, text) || is_restatement(&live, text) {
            if self.committed.is_empty() || is_restatement(&self.committed.join(" "), text) {
                self.committed.clear();
                self.partial = text.to_string();
            } else {
                self.partial = text.to_string();
            }
            return;
        }

        // Not a restatement: either Speechmatics' unfinalized tail after a
        // prefix final, or the first partial of a brand-new utterance after
        // a complete previous one (Parakeet).
        if self.looks_like_a_finished_turn() {
            self.committed.clear();
            self.partial = text.to_string();
        } else {
            self.partial = text.to_string();
        }
    }

    fn looks_like_a_finished_turn(&self) -> bool {
        let live = self.live();
        let words = live.split_whitespace().count();
        words >= 5 || live.trim().ends_with('?')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_growing_partials_then_a_whole_utterance_final() {
        let mut b = ChannelBuffer::default();
        b.push("what", false);
        b.push("what is your", false);
        b.push("what is your notice period", false);
        assert_eq!(b.live(), "what is your notice period");
        b.push("what is your notice period", true);
        assert_eq!(b.live(), "what is your notice period");
        assert!(b.partial.is_empty());
    }

    #[test]
    fn parakeet_next_utterance_does_not_keep_the_previous_question() {
        let mut b = ChannelBuffer::default();
        b.push("what is your notice period", true);
        b.push("can you walk me through a project", false);
        assert_eq!(b.live(), "can you walk me through a project");
    }

    #[test]
    fn speechmatics_prefix_finals_plus_a_tail_partial_join() {
        let mut b = ChannelBuffer::default();
        b.push("what is your", true);
        b.push("notice period", false);
        assert_eq!(b.live(), "what is your notice period");
        b.push("notice period", true);
        assert_eq!(b.live(), "what is your notice period");
    }

    #[test]
    fn speechmatics_new_question_after_a_complete_final_starts_fresh() {
        let mut b = ChannelBuffer::default();
        b.push("What is your notice period?", true);
        b.push("and how soon", false);
        assert_eq!(b.live(), "and how soon");
    }

    #[test]
    fn empty_and_whitespace_pushes_do_nothing() {
        let mut b = ChannelBuffer::default();
        b.push("  ", false);
        b.push("", true);
        assert!(b.live().is_empty());
    }
}
