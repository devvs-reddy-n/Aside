//! Rolling "do we understand the question well enough to start answering?"
//!
//! Trigger is **the question is sufficiently understood**, not **the final
//! transcript arrived**. Finals still matter: they confirm or correct.

use crate::audio::channel::Channel;
use crate::llm::trigger::looks_like_a_question;

/// Start a speculative answer once the live wording is at least this long.
/// Short enough that "what is your notice period" (5 words) qualifies;
/// long enough that a lone "what" does not.
pub const MIN_WORDS: usize = 5;

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Interviewer (system audio) questions keep the existing opener/`?` check.
/// The user's own speech is stricter: a spoken *answer* that starts with
/// "What I did was…" must not look like a question to Aside.
pub fn looks_like_a_user_question(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if !looks_like_a_question(trimmed) {
        return false;
    }
    let mut words = trimmed.split_whitespace();
    let first = words
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let second = words
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if first == "what"
        && matches!(
            second.as_str(),
            "i" | "i've" | "id" | "we" | "we've" | "happened" | "my"
        )
    {
        return trimmed.ends_with('?');
    }
    true
}

pub fn looks_like_question_from(channel: Channel, text: &str) -> bool {
    match channel {
        Channel::Mic => looks_like_a_user_question(text),
        Channel::System => looks_like_a_question(text),
    }
}

/// True once the live wording is a question *and* long enough that starting
/// the answer now is a reasonable bet. Finals skip the word-count floor so
/// a short "why?" still gets an answer when the speaker actually stops.
pub fn is_sufficiently_understood(channel: Channel, text: &str, is_final: bool) -> bool {
    if !looks_like_question_from(channel, text) {
        return false;
    }
    is_final || word_count(text) >= MIN_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_five_word_interviewer_question_is_enough_to_start() {
        assert!(is_sufficiently_understood(
            Channel::System,
            "what is your notice period",
            false,
        ));
    }

    #[test]
    fn a_lone_opener_is_not_enough_while_they_are_still_talking() {
        assert!(!is_sufficiently_understood(Channel::System, "what", false));
        assert!(!is_sufficiently_understood(
            Channel::System,
            "how did",
            false
        ));
    }

    #[test]
    fn a_short_final_why_still_counts() {
        assert!(is_sufficiently_understood(Channel::System, "why?", true));
    }

    #[test]
    fn the_user_answering_what_i_did_is_not_a_question_to_aside() {
        assert!(!looks_like_a_user_question(
            "what I did at my last job was lead the audio rewrite"
        ));
        assert!(!is_sufficiently_understood(
            Channel::Mic,
            "what I did at my last job was lead the audio rewrite",
            true,
        ));
    }

    #[test]
    fn the_user_asking_aside_a_real_question_does_count() {
        assert!(looks_like_a_user_question(
            "what's a good way to explain CAP theorem"
        ));
        assert!(looks_like_a_user_question(
            "can you remind me of my notice period"
        ));
        assert!(is_sufficiently_understood(
            Channel::Mic,
            "can you remind me of my notice period",
            false,
        ));
    }

    #[test]
    fn a_plain_statement_is_never_sufficient() {
        assert!(!is_sufficiently_understood(
            Channel::System,
            "thanks for coming in today everyone",
            true,
        ));
    }
}
