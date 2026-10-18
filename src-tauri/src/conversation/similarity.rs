//! Word-set similarity (Jaccard on word sets). `is_restatement` is the real
//! consumer today — `window_memory::live` and `conversation::channel_text`
//! both use it to tell a growing/corrected partial from a genuinely new
//! utterance.

use std::collections::HashSet;

fn tokens(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Jaccard similarity of the alphanumeric word sets: `|A ∩ B| / |A ∪ B|`.
/// Empty-vs-empty is `1.0` (they match); empty-vs-anything-else is `0.0`.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let a = tokens(a);
    let b = tokens(b);
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f64;
    let union = a.union(&b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// True when `new` is a growing (or slightly corrected) restatement of
/// `old` — the shape local Parakeet emits for partials — rather
/// than a fresh tail of unfinalized words (Speechmatics).
pub fn is_restatement(old: &str, new: &str) -> bool {
    let old = old.trim();
    let new = new.trim();
    if old.is_empty() || new.is_empty() {
        return false;
    }
    let o = old.to_lowercase();
    let n = new.to_lowercase();
    n.starts_with(&o) || o.starts_with(&n) || jaccard(old, new) >= 0.6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_questions_score_one() {
        assert!(
            (jaccard("what is your notice period", "what is your notice period") - 1.0).abs()
                < 1e-9
        );
    }

    #[test]
    fn empty_pairs_are_defined_rather_than_dividing_by_zero() {
        assert_eq!(jaccard("", ""), 1.0);
        assert_eq!(jaccard("", "hello"), 0.0);
        assert_eq!(jaccard("hello", ""), 0.0);
    }

    #[test]
    fn a_growing_parakeet_partial_is_a_restatement() {
        assert!(is_restatement("what is your", "what is your notice"));
        assert!(is_restatement("what is your notice", "what is your"));
    }

    #[test]
    fn a_speechmatics_tail_is_not_a_restatement_of_the_prefix() {
        assert!(!is_restatement("what is your", "notice period"));
    }
}
