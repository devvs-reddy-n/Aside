//! Regex-based fact extraction (`status.md` 7.8). See `extract`'s module
//! doc comment for scope and honest limitations.

pub mod briefing;
pub mod extract;
pub mod pack;

use extract::ExtractedFact;

/// Regex extract plus the visa lexicon. A spoken "actually STEM OPT"
/// must become a visa fact even when it does not match "my visa status is…".
/// Interviewer questions that merely mention a visa class are left alone.
pub fn facts_from_utterance(text: &str) -> Vec<ExtractedFact> {
    let mut facts = extract::extract(text);
    let Some(visa) = briefing::parse_visa(text) else {
        return facts;
    };
    if let Some(existing) = facts.iter_mut().find(|f| f.key == "visa_status") {
        existing.value = visa;
        return facts;
    }
    let lower = text.to_lowercase();
    let self_correction = lower.contains("actually")
        || lower.contains("i am")
        || lower.contains("i'm")
        || lower.contains("i mean")
        || lower.contains("my visa");
    if self_correction {
        facts.push(ExtractedFact {
            key: "visa_status".to_string(),
            value: visa,
            confidence: 0.85,
        });
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_i_mean_stem_opt_becomes_a_visa_fact() {
        let facts = facts_from_utterance("I mean STEM OPT");
        let visa = facts
            .iter()
            .find(|f| f.key == "visa_status")
            .expect("spoken 'I mean STEM OPT' must land as visa_status");
        assert_eq!(visa.value, "STEM OPT");
    }

    #[test]
    fn spoken_actually_stem_opt_becomes_a_visa_fact() {
        let facts = facts_from_utterance("actually STEM OPT");
        let visa = facts
            .iter()
            .find(|f| f.key == "visa_status")
            .expect("spoken correction must land as visa_status");
        assert_eq!(visa.value, "STEM OPT");
    }

    #[test]
    fn lexicon_upgrades_extracted_opt_to_stem_opt() {
        let facts = facts_from_utterance("I am on an OPT, actually STEM OPT");
        let visa = facts.iter().find(|f| f.key == "visa_status").unwrap();
        assert_eq!(visa.value, "STEM OPT");
    }

    #[test]
    fn an_interviewer_question_that_only_mentions_h1b_is_not_stored_as_our_visa() {
        let facts = facts_from_utterance("tell me about H-1B sponsorship at your last job");
        assert!(
            !facts.iter().any(|f| f.key == "visa_status"),
            "mentioning a visa class in a question must not overwrite the ledger: {facts:?}"
        );
    }
}
