//! Visa-class lexicon, lexicon + regex only, no LLM. The rest of this
//! module (parsing a whole typed "briefing" into facts and committing it)
//! was removed 2026-08-31 along with its one caller,
//! `commands::session::commit_briefing` — a leftover of the pre-Copilots
//! "Create Session" wizard, unreachable since that screen was replaced.
//! `parse_visa` survives because `facts::extract` still calls it directly
//! for any spoken/typed text, not just a dedicated briefing box.

/// Visa classes we recognise without calling a model.
pub fn parse_visa(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.contains("stem opt") || lower.contains("stem-opt") {
        return Some("STEM OPT".to_string());
    }
    if lower.contains("h-1b")
        || lower.contains("h1b")
        || lower.contains("h1-b")
        || lower.contains("h 1b")
    {
        return Some("H-1B".to_string());
    }
    if regex_word(&lower, "opt") {
        return Some("OPT".to_string());
    }
    None
}

fn regex_word(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric() && c != '-')
        .any(|w| w == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_opt_wins_the_lexicon_over_a_bare_opt_substring() {
        assert_eq!(
            parse_visa("I am on STEM OPT, not H-1B").as_deref(),
            Some("STEM OPT")
        );
        assert_eq!(parse_visa("currently on OPT").as_deref(), Some("OPT"));
        assert_eq!(parse_visa("need H1B sponsorship").as_deref(), Some("H-1B"));
    }
}
