//! Pure "does this look like a question worth answering" + cooldown logic.
//!
//! Deliberately just a regex/word-list fast-path — the equivalent of
//! Natively's `hasQuestionSignal` (`electron/llm/IntentClassifier.ts`, read
//! during this phase's research, see `llm/mod.rs`'s doc comment) — without
//! Natively's heavier local zero-shot ONNX classifier layered on top.
//! Proportionate to Phase 6's scope; a smarter classifier is a reasonable
//! future upgrade, not a gap hidden here.
//!
//! **Why not just check for a trailing `?`.** Aside's local Parakeet engine
//! (the default, no-API-key STT engine — see `stt/parakeet_local.rs`'s
//! module doc: "No punctuation/capitalization from this model") never
//! produces a `?` at all. Deepgram (`smart_format=true`) and Speechmatics do
//! add punctuation. A trigger that only checked for `?` would silently never
//! fire for anyone using the default engine — so this also matches on a
//! sentence *starting* with a common interrogative opener, which survives
//! having no punctuation.

/// Interrogative sentence openers, lowercase, checked as whole leading
/// words (not substrings) so "wherever" doesn't match "where". Covers wh-
/// words plus the auxiliary-verb-first openers ("do you", "can you", ...)
/// that make up the overwhelming majority of spoken questions.
const QUESTION_OPENERS: &[&str] = &[
    "what", "whats", "why", "whys", "how", "hows", "when", "whens", "where", "wheres", "who",
    "whos", "whom", "whose", "which", "can", "could", "would", "will", "should", "do", "does",
    "did", "is", "are", "was", "were", "have", "has", "had",
];

/// True if `text` reads as a question — either it ends with `?` (works for
/// engines that emit punctuation), or its first word is a recognized
/// interrogative opener (works even with no punctuation at all).
pub fn looks_like_a_question(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('?') {
        return true;
    }
    let first_word = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .replace('\'', "")
        .to_lowercase();
    QUESTION_OPENERS.contains(&first_word.as_str())
}

/// How close two user keypresses (or a local + global shortcut on the same
/// chord) may be and still count as one action. Speculative auto-answers
/// do not write this clock, so a real ⌘↩ can interrupt a guess immediately.
pub const USER_ACTION_DEBOUNCE_MS: u64 = 200;

/// True when a second user-initiated answer/screenshot landed so soon after
/// the last one that it is almost certainly the helper-focused local+global
/// double-fire, not a new intent. `last_user_started_ms == 0` means "never."
pub fn is_duplicate_user_action(last_user_started_ms: u64, now_ms: u64) -> bool {
    last_user_started_ms != 0
        && now_ms.saturating_sub(last_user_started_ms) < USER_ACTION_DEBOUNCE_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_question_mark_is_recognized() {
        assert!(looks_like_a_question("What's your biggest weakness?"));
        assert!(looks_like_a_question("really?"));
    }

    #[test]
    fn a_whats_opener_without_a_question_mark_still_counts() {
        assert!(looks_like_a_question("what's a good way to explain that"));
        assert!(looks_like_a_question("how's the team structured"));
    }

    #[test]
    fn a_wh_opener_with_no_punctuation_is_recognized() {
        // The exact case local Parakeet produces — no punctuation at all.
        assert!(looks_like_a_question("what is your biggest weakness"));
        assert!(looks_like_a_question("how did you handle that project"));
        assert!(looks_like_a_question("can you walk me through your resume"));
        assert!(looks_like_a_question("do you have experience with rust"));
    }

    #[test]
    fn a_plain_statement_is_not_a_question() {
        assert!(!looks_like_a_question("I led the audio pipeline rewrite"));
        assert!(!looks_like_a_question("thanks for coming in today"));
    }

    #[test]
    fn empty_or_whitespace_only_text_is_never_a_question() {
        assert!(!looks_like_a_question(""));
        assert!(!looks_like_a_question("   "));
    }

    #[test]
    fn a_word_that_merely_starts_with_an_opener_does_not_false_positive() {
        // "wherever" must not match "where" — this is why the check is on
        // the whole first word, not a substring/prefix test.
        assert!(!looks_like_a_question("wherever you go, I'll follow"));
        assert!(!looks_like_a_question("whatever works for you"));
    }

    #[test]
    fn leading_punctuation_or_capitalization_does_not_defeat_the_match() {
        assert!(looks_like_a_question(
            "\"What made you choose this career?\""
        ));
        assert!(looks_like_a_question("WHAT is your greatest strength"));
    }

    #[test]
    fn a_first_user_action_is_never_a_duplicate() {
        assert!(!is_duplicate_user_action(0, 100));
    }

    #[test]
    fn a_second_user_action_within_the_debounce_is_a_duplicate() {
        assert!(is_duplicate_user_action(1_000, 1_050));
    }

    #[test]
    fn a_second_user_action_after_the_debounce_is_not_a_duplicate() {
        assert!(!is_duplicate_user_action(
            1_000,
            1_000 + USER_ACTION_DEBOUNCE_MS
        ));
    }
}
