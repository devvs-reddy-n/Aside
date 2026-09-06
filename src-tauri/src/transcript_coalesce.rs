//! Same-ear notebook line joining (Layer 3).
//!
//! Engines still finalize around 1.5s of quiet. The notebook does **not**
//! start a new bubble for that. Same speaker continues for up to
//! [`NOTEBOOK_COALESCE_MS`] (5s) unless the other ear spoke in between.
//! A 1.5s pause only becomes a new line when Others start talking (channel
//! change). After 5s of the same ear with nobody else talking, a new bubble.

use crate::conversation::ack::is_ack_only;

/// Inclusive. Pause 2–4s on the same ear, resume, nobody else spoke → same bubble.
pub const NOTEBOOK_COALESCE_MS: u64 = 5_000;

pub fn should_continue_line(
    prev_channel: &str,
    next_channel: &str,
    gap_ms: u64,
    prev_is_session_latest: bool,
    prev_text: &str,
) -> bool {
    if prev_channel != next_channel {
        return false;
    }
    if !prev_is_session_latest {
        return false;
    }
    if gap_ms > NOTEBOOK_COALESCE_MS {
        return false;
    }
    if is_ack_only(prev_text) {
        return false;
    }
    true
}

pub fn merge_notebook_text(prev: &str, next: &str) -> String {
    let prev = prev.trim();
    let next = next.trim();
    if prev.is_empty() {
        return next.to_string();
    }
    if next.is_empty() {
        return prev.to_string();
    }
    let prev_l = prev.to_ascii_lowercase();
    let next_l = next.to_ascii_lowercase();
    if next_l.starts_with(&prev_l) && next.len() > prev.len() {
        return next.to_string();
    }
    // Disk already joined; the live fragment is a suffix. Keep the long line.
    // Equal repeats ("okay" + "okay") stay two words.
    if prev.len() > next.len() && prev_l.ends_with(&format!(" {next_l}")) {
        return prev.to_string();
    }
    format!("{prev} {next}")
}

use std::collections::HashSet;

/// Fuzzy echo only runs when both sides are at least this long. Short
/// "yeah"/"okay" after Others said the same word must still count as Me.
pub const ECHO_MIN_TOKENS: usize = 8;
pub const ECHO_MIN_CHARS: usize = 48;
/// Token-set Jaccard at or above this means the mic line is the speakers.
pub const ECHO_JACCARD: f64 = 0.85;
/// How many recent Others finals to compare against (Me can land first).
pub const ECHO_LOOKBACK: usize = 4;

/// Lowercase, drop punctuation, collapse spaces — for echo compare only.
pub fn normalize_echo_text(s: &str) -> String {
    let cleaned: String = s
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn echo_tokens(normalized: &str) -> Vec<&str> {
    if normalized.is_empty() {
        Vec::new()
    } else {
        normalized.split(' ').collect()
    }
}

fn is_echo_long_enough(tokens: &[&str], normalized: &str) -> bool {
    tokens.len() >= ECHO_MIN_TOKENS || normalized.len() >= ECHO_MIN_CHARS
}

/// Unique-token Jaccard. Empty vs empty is 0 so a blank line never matches.
pub fn token_jaccard(a: &[&str], b: &[&str]) -> f64 {
    let set_a: HashSet<&str> = a.iter().copied().collect();
    let set_b: HashSet<&str> = b.iter().copied().collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 0.0;
    }
    let inter = set_a.intersection(&set_b).count();
    let union = set_a.len() + set_b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// True when the mic line is almost certainly the speakers leaking in.
/// Short backchannels stay; near-duplicate long paragraphs drop.
pub fn is_likely_speaker_echo(candidate: &str, reference: &str) -> bool {
    let a = normalize_echo_text(candidate);
    let b = normalize_echo_text(reference);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let ta = echo_tokens(&a);
    let tb = echo_tokens(&b);
    if !is_echo_long_enough(&ta, &a) || !is_echo_long_enough(&tb, &b) {
        return false;
    }
    if a == b {
        return true;
    }
    if a.contains(&b) || b.contains(&a) {
        return true;
    }
    token_jaccard(&ta, &tb) >= ECHO_JACCARD
}

pub fn is_likely_speaker_echo_of_any<T: AsRef<str>>(candidate: &str, references: &[T]) -> bool {
    references
        .iter()
        .any(|r| is_likely_speaker_echo(candidate, r.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_same_channel_pause_stays_on_the_line() {
        assert!(should_continue_line(
            "mic",
            "mic",
            1_000,
            true,
            "I was thinking"
        ));
        assert!(should_continue_line(
            "mic",
            "mic",
            1_500,
            true,
            "I was thinking"
        ));
        assert!(should_continue_line(
            "mic",
            "mic",
            4_000,
            true,
            "I was thinking"
        ));
        assert!(should_continue_line(
            "mic",
            "mic",
            5_000,
            true,
            "I was thinking"
        ));
    }

    #[test]
    fn a_gap_over_five_seconds_is_a_new_line() {
        assert!(!should_continue_line(
            "mic",
            "mic",
            5_001,
            true,
            "I was thinking"
        ));
    }

    #[test]
    fn same_tick_same_speaker_stays_one_bubble() {
        assert!(should_continue_line(
            "mic",
            "mic",
            0,
            true,
            "I was thinking"
        ));
    }

    #[test]
    fn channel_change_is_a_new_line_at_any_gap() {
        assert!(!should_continue_line("mic", "system", 0, true, "hello"));
        assert!(!should_continue_line("mic", "system", 200, true, "hello"));
    }

    #[test]
    fn other_party_in_between_blocks_join() {
        assert!(!should_continue_line("mic", "mic", 400, false, "hello"));
    }

    #[test]
    fn ack_only_previous_line_is_not_extended() {
        assert!(!should_continue_line("mic", "mic", 400, true, "Yes."));
        assert!(!should_continue_line("mic", "mic", 200, true, "Okay."));
        assert!(!should_continue_line(
            "system", "system", 400, true, "got it"
        ));
    }

    #[test]
    fn coalesce_window_of_zero_never_joins() {
        // Anti-tautology: if NOTEBOOK_COALESCE_MS is ignored and we only
        // check same-channel, this would still pass. A 5s+1ms gap must split.
        assert!(!should_continue_line(
            "mic",
            "mic",
            NOTEBOOK_COALESCE_MS + 1,
            true,
            "hello"
        ));
    }

    #[test]
    fn merge_appends_a_repeated_okay_instead_of_dropping_it() {
        assert_eq!(merge_notebook_text("okay", "okay"), "okay okay");
    }

    #[test]
    fn merge_keeps_a_growing_restatement() {
        assert_eq!(
            merge_notebook_text("I work on Kubernetes", "I work on Kubernetes and Helm"),
            "I work on Kubernetes and Helm"
        );
    }

    #[test]
    fn merge_joins_a_mid_thought_continuation() {
        assert_eq!(
            merge_notebook_text("I was thinking", "about Kubernetes"),
            "I was thinking about Kubernetes"
        );
    }

    #[test]
    fn merge_absorbs_a_suffix_already_on_the_line() {
        assert_eq!(
            merge_notebook_text("I was thinking about Kubernetes", "about Kubernetes"),
            "I was thinking about Kubernetes"
        );
    }

    #[test]
    fn others_same_ear_within_five_seconds_stays_one_line() {
        assert!(should_continue_line(
            "system",
            "system",
            1_500,
            true,
            "Can you explain Kubernetes"
        ));
        assert!(should_continue_line(
            "system",
            "system",
            5_000,
            true,
            "Can you explain Kubernetes"
        ));
        assert!(!should_continue_line(
            "system",
            "system",
            5_001,
            true,
            "Can you explain Kubernetes"
        ));
    }

    #[test]
    fn me_speaking_in_between_blocks_others_join() {
        assert!(!should_continue_line(
            "system", "system", 400, false, "hello"
        ));
        assert!(!should_continue_line("system", "mic", 200, true, "hello"));
    }

    const FORTY_CENTS: &str = "about a dollar and thirty cents forty cents with kimmy on the phone and we can probably do that if it still makes sense for the budget this quarter";
    const OR_CENTS: &str = "about a dollar and thirty cents or cents with kimmy on the phone and we can probably do that if it still makes sense for the budget this quarter";
    const LONG_K8S: &str =
        "Can you walk me through Kubernetes and how you used it on STEM OPT last year?";

    #[test]
    fn speaker_echo_drops_the_forty_or_cents_near_duplicate() {
        let a_norm = normalize_echo_text(FORTY_CENTS);
        let b_norm = normalize_echo_text(OR_CENTS);
        let a = echo_tokens(&a_norm);
        let b = echo_tokens(&b_norm);
        assert!(
            token_jaccard(&a, &b) >= ECHO_JACCARD,
            "screenshot pair must clear the Jaccard bar, got {}",
            token_jaccard(&a, &b)
        );
        assert!(is_likely_speaker_echo(OR_CENTS, FORTY_CENTS));
        assert!(is_likely_speaker_echo(FORTY_CENTS, OR_CENTS));
    }

    #[test]
    fn speaker_echo_keeps_a_short_yeah_after_the_same_yeah() {
        assert!(!is_likely_speaker_echo("yeah", "yeah"));
        assert!(!is_likely_speaker_echo("Okay.", "okay"));
        assert!(!is_likely_speaker_echo("yeah", "Yeah, sure."));
    }

    #[test]
    fn speaker_echo_matches_a_long_exact_sentence() {
        assert!(is_likely_speaker_echo(LONG_K8S, LONG_K8S));
        assert!(is_likely_speaker_echo(
            "can you walk me through kubernetes and how you used it on stem opt last year",
            LONG_K8S
        ));
        assert!(!is_likely_speaker_echo(
            "I use Kubernetes at work on STEM OPT",
            LONG_K8S
        ));
        assert!(!is_likely_speaker_echo("", LONG_K8S));
        assert!(!is_likely_speaker_echo("hello", "world"));
    }

    #[test]
    fn speaker_echo_lookback_matches_any_recent_others_line() {
        let older = LONG_K8S.to_string();
        let latest = "What about Helm in that same role?".to_string();
        assert!(is_likely_speaker_echo_of_any(
            LONG_K8S,
            &[older.clone(), latest.clone()]
        ));
        assert!(!is_likely_speaker_echo_of_any(
            "I use Helm at work on STEM OPT",
            &[older, latest]
        ));
    }
}
