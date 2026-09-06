//! Drops known silence-hallucination phrases from **local** STT output.
//!
//! Cloud engines (Deepgram smart_format, Speechmatics enhanced) already
//! suppress silence-garbage; this list must not run on their transcripts.
//! Local speech models sometimes invent text from silence or background
//! noise rather than reporting nothing — both reference apps hit this
//! independently and both fixed it the same way: an exact-match phrase list,
//! no fuzzy matching, no ML classifier. `references/Repo1-Natively`'s and
//! `references/Repo2-OpenCluely`'s lists barely overlap (`you`, `bye` are the
//! only shared entries), so this union covers what either one alone would
//! miss. Natively additionally regex-matches bracket tokens like `[Noise]` or
//! `[BLANK_AUDIO]`.
//!
//! **Known, accepted limitation** (flagged by an adversarial review, not
//! overlooked): because matching is against the *entire* normalized
//! utterance, a real short utterance that happens to equal a listed phrase
//! is indistinguishable from a hallucination and gets dropped — "Okay.",
//! "So?", "You?", and, worst-case, a genuine "Bye." ending a call. Both
//! reference apps carry the identical trade-off (inherited, not introduced
//! here); a real fix needs more signal than the text alone — e.g. the
//! model's own per-segment confidence score, not available from every
//! engine — which is out of scope for this phase. Accepted the same way
//! `audio/vad.rs`'s documented steady-noise limitation was: real, bounded,
//! and worse to "fix" naively than to leave alone.

use std::sync::LazyLock;

use regex::Regex;

/// Union of `references/Repo1-Natively`'s local-STT `EXACT_BLOCKS` and
/// `references/Repo2-OpenCluely/src/services/speech.service.js`'s
/// `HALLUCINATIONS`. Matched against text that has already been lowercased,
/// had punctuation stripped, and been trimmed.
const HALLUCINATED_PHRASES: &[&str] = &[
    // Natively
    "music",
    "applause",
    "inaudible",
    "thank you for watching",
    "thanks for watching",
    "you",
    "bye",
    // OpenCluely
    "thank you so much for watching",
    "please subscribe",
    "like and subscribe",
    "bye bye",
    "okay",
    "ok",
    "so",
    "the end",
    "subtitles by the amara org community",
];

/// Matches Natively's `BRACKET_TOKEN_RE` — a whole string of the form
/// `[anything]`, e.g. `[Noise]`, `[BLANK_AUDIO]`, `[MUSIC PLAYING]`. Checked
/// against the *original* text, since bracket tokens are their own signal —
/// stripping punctuation first would destroy the brackets that identify them.
static BRACKET_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[.*\]$").expect("bracket token regex is valid"));

/// Lowercases, strips punctuation to spaces, and collapses/trims whitespace —
/// the same normalization OpenCluely applies before comparing against its
/// phrase list, so `"Thank you for watching!"` and `"thank you for watching"`
/// are recognized as the same hallucination.
fn normalize(text: &str) -> String {
    let lowered = text.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_space = false;
    for c in lowered.chars() {
        let is_word_char = c.is_alphanumeric();
        if is_word_char {
            out.push(c);
            last_was_space = false;
        } else if !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim().to_string()
}

/// True if `text` is a known hallucination or effectively empty.
pub fn is_hallucination(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "..." || trimmed == "." {
        return true;
    }
    if BRACKET_TOKEN.is_match(trimmed) {
        return true;
    }
    let normalized = normalize(text);
    if normalized.is_empty() {
        return true;
    }
    HALLUCINATED_PHRASES.contains(&normalized.as_str())
}

/// Returns `Some(text)` unchanged (not normalized — normalization is only for
/// comparison, never shown to the user) when it's real transcribed speech,
/// `None` when it's a hallucination that should be dropped entirely.
pub fn clean(text: &str) -> Option<String> {
    if is_hallucination(text) {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_speech_passes_through_unchanged() {
        assert_eq!(
            clean("so tell me about a time you led a project"),
            Some("so tell me about a time you led a project".to_string())
        );
    }

    #[test]
    fn empty_and_whitespace_only_are_hallucinations() {
        assert!(is_hallucination(""));
        assert!(is_hallucination("   "));
        assert!(is_hallucination("..."));
        assert!(is_hallucination("."));
    }

    #[test]
    fn natively_phrases_are_caught() {
        assert!(is_hallucination("Thank you for watching"));
        assert!(is_hallucination("thank you for watching"));
        assert!(is_hallucination("You"));
        assert!(is_hallucination("bye"));
    }

    #[test]
    fn opencluely_phrases_are_caught() {
        assert!(is_hallucination("please subscribe"));
        assert!(is_hallucination("like and subscribe"));
        assert!(is_hallucination("Okay"));
        assert!(is_hallucination("the end."));
    }

    #[test]
    fn bracket_tokens_are_caught_dynamically() {
        // Not in either phrase list — caught by the regex, not a lookup.
        assert!(is_hallucination("[Noise]"));
        assert!(is_hallucination("[BLANK_AUDIO]"));
        assert!(is_hallucination("[MUSIC PLAYING]"));
    }

    #[test]
    fn a_bracket_token_embedded_mid_sentence_is_not_caught() {
        // The regex requires the WHOLE trimmed string to be one bracket
        // token — real speech that happens to mention brackets must survive.
        assert!(!is_hallucination("he said [laughs] that's hilarious"));
    }

    #[test]
    fn punctuation_and_case_do_not_let_a_hallucination_slip_through() {
        assert!(is_hallucination("THANK YOU FOR WATCHING!!!"));
        assert!(is_hallucination("Bye bye."));
    }

    #[test]
    fn words_that_are_hallucinations_alone_survive_inside_real_sentences() {
        // "so" and "okay" are hallucination-only when they're the WHOLE
        // utterance — a real sentence merely containing them must not be
        // dropped, or the filter would eat ordinary speech.
        assert!(!is_hallucination("so I think we should go with option two"));
        assert!(!is_hallucination("okay let's move on to the next question"));
    }

    #[test]
    fn a_single_real_word_is_not_treated_as_a_hallucination() {
        // Guards against the normalizer or phrase list being too aggressive.
        assert!(!is_hallucination("scalability"));
    }

    #[test]
    fn a_genuine_short_utterance_that_matches_the_phrase_list_is_still_dropped() {
        // Documents the accepted limitation from this module's doc comment,
        // executably: a real "Bye." ending a call is indistinguishable from
        // a local model hallucinating the word from silence, and is dropped either
        // way. Not a bug to fix here — a regression guard so this trade-off
        // stays a visible, tested decision rather than a silent surprise if
        // someone "fixes" it without realizing what breaks.
        assert!(is_hallucination("Bye."));
        assert!(is_hallucination("Okay."));
    }
}
