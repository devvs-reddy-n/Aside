//! Pure memory-shape helpers for the single growing-summary design (owner
//! spec, 2026-08-29 simplification of the earlier two-layer recent/
//! historical design). All the actual *content* in `summary` is produced
//! asynchronously by `background.rs`'s cheap second model — see that
//! module's doc comment for why a compressor that only ever sees the raw
//! completed transcript (never the visible answer the main model suggested)
//! is what keeps an unspoken AI suggestion from ever being remembered as
//! something a real participant said. This module stays synchronous and
//! network-free: just plain append plus the small amount of de-duping
//! `merge_db_facts` still needs for document/briefing-derived facts that
//! never go through the compressor at all.

use super::types::{ConversationWindow, RawTurn, SessionMemory};

pub fn normalize(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Exact match (case/whitespace-insensitive) counts as a duplicate, and so
/// does simple substring containment either direction — cheap and
/// deliberately blunt, meant to stop obvious noise, not to be a real
/// similarity model.
fn already_has(list: &[String], item: &str) -> bool {
    let n = normalize(item);
    if n.is_empty() {
        return true;
    }
    list.iter().any(|existing| {
        let e = normalize(existing);
        e == n || e.contains(&n) || n.contains(&e)
    })
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return;
    }
    if already_has(list, trimmed) {
        return;
    }
    list.push(trimmed.to_string());
}

/// Appends `item` to `summary` — plain, unconditional, no cap and no
/// eviction. The owner's explicit spec: every completed window's summary
/// joins the one growing list, in order, for the rest of the session.
/// `prompt::build_prompt`'s own budget ladder is the only place `summary`
/// ever shrinks (trimming the oldest entries first if the assembled prompt
/// gets too large), not this function.
pub fn push_summary(summary: &mut Vec<String>, item: String) {
    summary.push(item);
}

/// Merges DB-backed facts (already loaded by the caller — this module stays
/// pure, no `rusqlite`) into `memory.facts` — a fixed, small key/value set
/// (resume/briefing-derived facts like visa status or notice period, one row
/// per key per `storage::facts`'s own `UNIQUE (session_id, key)` schema
/// constraint), never a window summary, so it never goes through
/// `push_summary`. Same de-dup as everything else in this module; no cap
/// needed since the underlying fact set is inherently small and bounded by
/// distinct fact keys, not by session length.
pub fn merge_db_facts(memory: &mut SessionMemory, items: impl IntoIterator<Item = String>) {
    for item in items {
        push_unique(&mut memory.facts, &item);
    }
}

/// Take the current open turns as a frozen window and leave `open` empty
/// so later speech cannot land in both windows.
pub fn freeze_open(open: &mut Vec<RawTurn>, start_ms: u64, end_ms: u64) -> ConversationWindow {
    ConversationWindow {
        turns: std::mem::take(open),
        start_ms,
        end_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_memory::types::Speaker;

    fn turn(speaker: Speaker, text: &str) -> RawTurn {
        RawTurn {
            speaker,
            text: text.to_string(),
            at_ms: 0,
        }
    }

    #[test]
    fn freeze_open_never_leaves_a_turn_in_both_windows() {
        let mut open = vec![
            turn(Speaker::Other, "first question"),
            turn(Speaker::Me, "okay"),
        ];
        let frozen = freeze_open(&mut open, 10, 20);
        open.push(turn(Speaker::Other, "next window only"));
        assert_eq!(frozen.turns.len(), 2);
        assert_eq!(frozen.turns[0].text, "first question");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].text, "next window only");
        assert!(frozen.turns.iter().all(|t| t.text != "next window only"));
    }

    #[test]
    fn normalize_collapses_case_and_spaces() {
        assert_eq!(
            normalize("  Databricks   AND   Kafka "),
            "databricks and kafka"
        );
    }

    #[test]
    fn push_summary_appends_in_order_with_no_cap() {
        let mut summary = Vec::new();
        for i in 0..25 {
            push_summary(&mut summary, format!("window-{i}"));
        }
        assert_eq!(
            summary.len(),
            25,
            "no eviction — every window's summary must survive"
        );
        assert_eq!(summary.first().unwrap(), "window-0");
        assert_eq!(summary.last().unwrap(), "window-24");
    }

    #[test]
    fn merge_db_facts_appends_and_dedups_against_existing_memory() {
        let mut memory = SessionMemory {
            facts: vec!["notice_period: two weeks".to_string()],
            summary: vec![],
        };
        merge_db_facts(
            &mut memory,
            vec![
                "notice_period: two weeks".to_string(),
                "visa_status: STEM OPT".to_string(),
            ],
        );
        assert_eq!(
            memory.facts.len(),
            2,
            "db facts land, duplicates skipped: {:?}",
            memory.facts
        );
        assert!(memory.facts.iter().any(|f| f.contains("STEM OPT")));
    }
}
