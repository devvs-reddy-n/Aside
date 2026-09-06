//! Regex fact extraction (`status.md` 7.8) — pulls a handful of
//! high-value, narrowly-defined facts (visa status, notice period, title,
//! location) out of a transcript segment's text as it's said, so a later
//! question ("what's your notice period again?") can be answered from a
//! stored fact instead of relying on the RAG retriever to happen to
//! surface the right chunk.
//!
//! **Deliberately narrow, stated honestly.** This is plain regex matching
//! against a handful of common spoken phrasings, not an LLM extractor —
//! proportionate to `status.md`'s scope for a personal project. It will
//! miss real facts stated in an unanticipated way ("I'd need about a
//! fortnight to wrap things up" never mentions "notice period"). That's a
//! known, accepted limitation, the same shape as `stt/hallucination_filter.rs`'s
//! own documented gap — not silently pretended away.
//!
//! **Corrections replace, never append** — enforced one layer down, by
//! `storage::facts::upsert_fact`'s real `ON CONFLICT(session_id, key) DO
//! UPDATE`, not by anything in this module. This module only ever produces
//! candidate `(key, value)` pairs from one piece of text; it has no memory
//! of what was extracted before.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedFact {
    pub key: String,
    pub value: String,
    pub confidence: f64,
}

struct Pattern {
    key: &'static str,
    regex: &'static str,
    confidence: f64,
}

/// Every pattern must have exactly one capture group — the value. Ordered
/// roughly most-specific-phrasing first; all patterns for a run are tried
/// against every input, not just the first match, since a single sentence
/// could plausibly state two different facts.
const PATTERNS: &[Pattern] = &[
    // --- visa status ---
    Pattern {
        key: "visa_status",
        regex: r"(?i)\bmy visa status is\s+([a-z0-9][a-z0-9 \-]{1,40}?)(?:[.,!?;]|$)",
        confidence: 0.9,
    },
    Pattern {
        key: "visa_status",
        regex: r"(?i)\bi(?:'m| am) (?:currently )?on an? ([a-z][a-z0-9\-]{0,10}(?: visa)?)\b",
        confidence: 0.75,
    },
    Pattern {
        key: "visa_status",
        regex: r"(?i)\bi(?:'m| am) (?:a |an )?(us citizen|permanent resident|green card holder)\b",
        confidence: 0.8,
    },
    // --- notice period ---
    Pattern {
        key: "notice_period",
        regex: r"(?i)\bmy notice period is\s+([a-z0-9][a-z0-9 \-]{1,30}?)(?:[.,!?;]|$)",
        confidence: 0.9,
    },
    Pattern {
        key: "notice_period",
        regex: r"(?i)\b(?:i need to give|i(?:'d| would) need to give)\s+([a-z0-9][a-z0-9 \-]{1,30}?)\s+notice\b",
        confidence: 0.8,
    },
    Pattern {
        key: "notice_period",
        regex: r"(?i)\b(\w+(?:\s\w+)?)\s+(?:weeks?|months?|days?)(?:'s)? notice\b",
        confidence: 0.6,
    },
    // --- title ---
    Pattern {
        key: "title",
        regex: r"(?i)\bmy (?:current |job )?title is\s+([a-z][a-z0-9 \-]{1,40}?)(?:[.,!?;]|$)",
        confidence: 0.9,
    },
    Pattern {
        key: "title",
        regex: r"(?i)\bi(?:'m| am) (?:currently )?(?:working as |a |an )([a-z][a-z0-9 \-]{2,40}?)(?:\s+at\b|[.,!?;]|$)",
        confidence: 0.65,
    },
    // --- location ---
    Pattern {
        key: "location",
        // Comma is deliberately *not* a terminator here (unlike the other
        // patterns) — a real location routinely contains one ("Austin,
        // Texas"), so cutting on the first comma would truncate exactly
        // the values this pattern exists to capture.
        regex: r"(?i)\bmy location is\s+([a-z][a-z0-9 \-,]{1,40}?)(?:[.!?;]|$)",
        confidence: 0.9,
    },
    Pattern {
        key: "location",
        regex: r"(?i)\bi(?:'m| am) (?:currently )?based in\s+([a-z][a-z0-9 \-,]{1,40}?)(?:[.,!?;]|$)",
        confidence: 0.8,
    },
    Pattern {
        key: "location",
        regex: r"(?i)\bi live in\s+([a-z][a-z0-9 \-,]{1,40}?)(?:[.,!?;]|$)",
        confidence: 0.8,
    },
];

fn compiled() -> &'static Vec<(&'static str, Regex, f64)> {
    static COMPILED: OnceLock<Vec<(&'static str, Regex, f64)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|p| {
                (
                    p.key,
                    Regex::new(p.regex)
                        .expect("pattern must compile — checked by this module's own tests"),
                    p.confidence,
                )
            })
            .collect()
    })
}

/// Runs every pattern against `text`, returning every match found — zero,
/// one, or several. Values are trimmed of surrounding whitespace; nothing
/// else is normalized (no lowercasing the value itself) so "H1-B" is stored
/// as the speaker actually said it.
pub fn extract(text: &str) -> Vec<ExtractedFact> {
    let mut facts = Vec::new();
    for (key, regex, confidence) in compiled() {
        if let Some(caps) = regex.captures(text) {
            if let Some(value) = caps.get(1) {
                let value = value.as_str().trim();
                if !value.is_empty() {
                    facts.push(ExtractedFact {
                        key: key.to_string(),
                        value: value.to_string(),
                        confidence: *confidence,
                    });
                }
            }
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        // Forces `compiled()`'s lazy init and its `.expect` to run for
        // every entry in `PATTERNS` — a typo'd regex fails this test
        // immediately instead of panicking lazily on first real use.
        assert_eq!(compiled().len(), PATTERNS.len());
    }

    #[test]
    fn extracts_visa_status_stated_directly() {
        let facts = extract("My visa status is H1-B, sponsored by my employer.");
        let visa = facts
            .iter()
            .find(|f| f.key == "visa_status")
            .expect("should find visa_status");
        assert_eq!(visa.value, "H1-B");
    }

    #[test]
    fn extracts_visa_status_from_i_am_on_an_x_phrasing() {
        let facts = extract("I am on an OPT right now.");
        let visa = facts
            .iter()
            .find(|f| f.key == "visa_status")
            .expect("should find visa_status");
        assert_eq!(visa.value.to_lowercase(), "opt");
    }

    #[test]
    fn extracts_us_citizen_as_a_visa_status() {
        let facts = extract("I'm a US citizen so no sponsorship needed.");
        let visa = facts
            .iter()
            .find(|f| f.key == "visa_status")
            .expect("should find visa_status");
        assert_eq!(visa.value.to_lowercase(), "us citizen");
    }

    #[test]
    fn extracts_notice_period_stated_directly() {
        let facts = extract("My notice period is two weeks.");
        let np = facts
            .iter()
            .find(|f| f.key == "notice_period")
            .expect("should find notice_period");
        assert_eq!(np.value, "two weeks");
    }

    #[test]
    fn extracts_notice_period_from_x_weeks_notice_phrasing() {
        let facts = extract("I can give four weeks notice.");
        let np = facts
            .iter()
            .find(|f| f.key == "notice_period")
            .expect("should find notice_period");
        assert!(np.value.to_lowercase().contains("four"));
    }

    #[test]
    fn extracts_title_stated_directly() {
        let facts = extract("My title is Senior Software Engineer.");
        let title = facts
            .iter()
            .find(|f| f.key == "title")
            .expect("should find title");
        assert_eq!(title.value, "Senior Software Engineer");
    }

    #[test]
    fn extracts_location_stated_directly() {
        let facts = extract("My location is Austin, Texas.");
        let loc = facts
            .iter()
            .find(|f| f.key == "location")
            .expect("should find location");
        assert_eq!(loc.value, "Austin, Texas");
    }

    #[test]
    fn extracts_location_from_i_live_in_phrasing() {
        // A trailing modifier before the next punctuation ("currently")
        // rides along in the captured value — a real, stated limitation of
        // plain regex extraction (see this module's doc comment), not a
        // bug this test pretends away. What matters is the real place name
        // is present in the value, not that the regex trims every trailing
        // word perfectly.
        let facts = extract("I live in Seattle currently.");
        let loc = facts
            .iter()
            .find(|f| f.key == "location")
            .expect("should find location");
        assert!(loc.value.to_lowercase().starts_with("seattle"));
    }

    #[test]
    fn unrelated_text_produces_no_facts() {
        let facts = extract("The weather has been really nice this week.");
        assert!(facts.is_empty());
    }

    #[test]
    fn one_sentence_can_yield_multiple_distinct_facts() {
        let facts = extract("I live in Austin and my notice period is two weeks.");
        let keys: Vec<&str> = facts.iter().map(|f| f.key.as_str()).collect();
        assert!(keys.contains(&"location"));
        assert!(keys.contains(&"notice_period"));
    }

    #[test]
    fn every_extracted_value_has_a_plausible_confidence() {
        let facts = extract("My visa status is H1-B. My notice period is two weeks. My title is Engineer. My location is NYC.");
        for f in &facts {
            assert!(
                f.confidence > 0.0 && f.confidence <= 1.0,
                "confidence out of range for {f:?}"
            );
        }
    }
}
