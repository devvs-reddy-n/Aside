//! Whole-turn acknowledgement filter.
//!
//! "okay" alone must not fire an answer. "Okay, but what is your notice
//! period?" still must. We never strip those words from the middle of a
//! real sentence — we only reject a turn that is *nothing but* acks.

const ACK_PHRASES: &[&str] = &[
    "thank you",
    "sounds good",
    "got it",
    "makes sense",
    "uh-huh",
    "uh huh",
    "mm-hmm",
    "mm hmm",
    "mmhmm",
    "all right",
    "alright",
    "okay",
    "ok",
    "right",
    "perfect",
    "yeah",
    "yep",
    "yes",
    "cool",
    "sure",
    "thanks",
    "go ahead",
    "mhm",
];

fn normalize(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if ch == '-' {
            out.push('-');
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when the entire turn is one or more ack phrases and nothing else.
pub fn is_ack_only(text: &str) -> bool {
    let mut rest = normalize(text);
    if rest.is_empty() {
        return false;
    }
    let mut progressed = true;
    while progressed && !rest.is_empty() {
        progressed = false;
        let mut phrases: Vec<&'static str> = ACK_PHRASES.to_vec();
        phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));
        for phrase in phrases {
            if rest == phrase {
                return true;
            }
            let prefix = format!("{phrase} ");
            if rest.starts_with(&prefix) {
                rest = rest[prefix.len()..].to_string();
                progressed = true;
                break;
            }
        }
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_okay_is_ack_only() {
        assert!(is_ack_only("okay"));
        assert!(is_ack_only("Ok."));
        assert!(is_ack_only("yeah, sure, thanks"));
        assert!(is_ack_only("Got it."));
        assert!(is_ack_only("mmhmm"));
        assert!(is_ack_only("uh-huh"));
        assert!(is_ack_only("thank you"));
        assert!(is_ack_only("sounds good"));
    }

    #[test]
    fn a_real_question_that_starts_with_okay_still_sends() {
        assert!(!is_ack_only("Okay, but what is your notice period?"));
        assert!(!is_ack_only("right, so tell me about CAP theorem"));
        assert!(!is_ack_only("what is your notice period"));
    }

    #[test]
    fn every_listed_acknowledgement_is_ack_only() {
        for phrase in ACK_PHRASES {
            assert!(
                is_ack_only(phrase),
                "{phrase:?} is on the list so it must be ack-only"
            );
        }
    }
}
