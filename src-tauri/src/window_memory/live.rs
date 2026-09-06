//! Per-speaker committed and interim transcript for Command+Enter.
//!
//! Finals are stored as turns. The latest valid interim is included at
//! freeze so the last spoken words are not lost while STT is still open.
//! Speech that arrives after the freeze belongs to the next window.
//! A late final of an already-frozen utterance is not duplicated.

use crate::conversation::similarity::is_restatement;

use super::merge::freeze_open;
use super::types::{ConversationWindow, RawTurn, Speaker};

#[derive(Debug, Clone, Default)]
pub struct SpeakerLive {
    pub seq: u64,
    pub interim: String,
    pub interim_at_ms: u64,
    pub frozen_seq: Option<u64>,
    pub frozen_text: String,
}

fn last_same_speaker(open: &[RawTurn], speaker: Speaker) -> Option<&RawTurn> {
    open.iter().rev().find(|t| t.speaker == speaker)
}

#[derive(Debug, Clone, Default)]
pub struct LiveState {
    pub me: SpeakerLive,
    pub other: SpeakerLive,
}

impl LiveState {
    pub fn speaker_mut(&mut self, speaker: Speaker) -> &mut SpeakerLive {
        match speaker {
            Speaker::Me => &mut self.me,
            Speaker::Other => &mut self.other,
        }
    }

    pub fn speaker(&self, speaker: Speaker) -> &SpeakerLive {
        match speaker {
            Speaker::Me => &self.me,
            Speaker::Other => &self.other,
        }
    }
}

fn push_turn(open: &mut Vec<RawTurn>, speaker: Speaker, text: &str, at_ms: u64) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if let Some(last) = open.last() {
        if last.speaker == speaker && is_restatement(&last.text, text) {
            open.last_mut().unwrap().text = text.to_string();
            open.last_mut().unwrap().at_ms = at_ms;
            return;
        }
    }
    open.push(RawTurn {
        speaker,
        text: text.to_string(),
        at_ms,
    });
}

/// Apply one STT event to the open window.
pub fn apply_event(
    open: &mut Vec<RawTurn>,
    live: &mut LiveState,
    speaker: Speaker,
    text: &str,
    is_final: bool,
    at_ms: u64,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let slot = live.speaker_mut(speaker);

    if !is_final {
        if slot.interim.is_empty() {
            slot.seq = slot.seq.saturating_add(1);
        } else if !is_restatement(&slot.interim, trimmed) && !is_restatement(trimmed, &slot.interim)
        {
            push_turn(open, speaker, &slot.interim, at_ms);
            slot.seq = slot.seq.saturating_add(1);
        }
        slot.interim = trimmed.to_string();
        slot.interim_at_ms = at_ms;
        return;
    }

    if !slot.frozen_text.is_empty() && is_restatement(&slot.frozen_text, trimmed) {
        let much_longer =
            trimmed.chars().count() > slot.frozen_text.chars().count().saturating_add(12);
        slot.interim.clear();
        slot.interim_at_ms = 0;
        slot.frozen_text.clear();
        slot.frozen_seq = None;
        slot.seq = slot.seq.saturating_add(1);
        if much_longer {
            push_turn(open, speaker, trimmed, at_ms);
        }
        return;
    }

    push_turn(open, speaker, trimmed, at_ms);
    slot.interim.clear();
    slot.interim_at_ms = 0;
    slot.seq = slot.seq.saturating_add(1);
}

/// Freeze committed turns plus any live interim that is not already a
/// committed restatement. Later events start the next window.
pub fn freeze_with_live(
    open: &mut Vec<RawTurn>,
    live: &mut LiveState,
    start_ms: u64,
    end_ms: u64,
) -> ConversationWindow {
    let mut extras = Vec::new();
    for speaker in [Speaker::Me, Speaker::Other] {
        let slot = live.speaker_mut(speaker);
        let interim = slot.interim.trim().to_string();
        if interim.is_empty() {
            continue;
        }
        let already =
            last_same_speaker(open, speaker).is_some_and(|t| is_restatement(&t.text, &interim));
        if !already {
            extras.push(RawTurn {
                speaker,
                text: interim.clone(),
                at_ms: if slot.interim_at_ms == 0 {
                    end_ms
                } else {
                    slot.interim_at_ms
                },
            });
        }
        slot.frozen_seq = Some(slot.seq);
        slot.frozen_text = interim;
        slot.interim.clear();
        slot.interim_at_ms = 0;
    }
    open.extend(extras);
    open.sort_by_key(|t| t.at_ms);
    freeze_open(open, start_ms, end_ms)
}

/// Turns the debug preview would show: committed plus current interims.
pub fn preview_turns(open: &[RawTurn], live: &LiveState, now_ms: u64) -> Vec<RawTurn> {
    let mut turns = open.to_vec();
    for speaker in [Speaker::Me, Speaker::Other] {
        let interim = live.speaker(speaker).interim.trim();
        if interim.is_empty() {
            continue;
        }
        let already =
            last_same_speaker(&turns, speaker).is_some_and(|t| is_restatement(&t.text, interim));
        if !already {
            turns.push(RawTurn {
                speaker,
                text: interim.to_string(),
                at_ms: now_ms,
            });
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(speaker: Speaker, text: &str) -> RawTurn {
        RawTurn {
            speaker,
            text: text.to_string(),
            at_ms: 0,
        }
    }

    #[test]
    fn freeze_includes_the_latest_interim_without_waiting_for_a_final() {
        let mut open = vec![turn(Speaker::Other, "Tell me about your current project.")];
        let mut live = LiveState::default();
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "And what do you expect from this role",
            false,
            20,
        );
        let frozen = freeze_with_live(&mut open, &mut live, 1, 30);
        assert!(
            frozen
                .turns
                .iter()
                .any(|t| t.text.contains("expect from this role")),
            "interim must be in the frozen window: {:?}",
            frozen.turns
        );
        assert!(open.is_empty());
    }

    #[test]
    fn overlapping_interims_do_not_duplicate_a_committed_other_line() {
        let mut open = vec![turn(Speaker::Other, "Tell me about your project.")];
        let mut live = LiveState::default();
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "Tell me about your project",
            false,
            10,
        );
        apply_event(&mut open, &mut live, Speaker::Me, "Sure.", false, 12);
        let frozen = freeze_with_live(&mut open, &mut live, 1, 20);
        let others = frozen
            .turns
            .iter()
            .filter(|t| t.speaker == Speaker::Other)
            .count();
        assert_eq!(others, 1, "Other must appear once: {:?}", frozen.turns);
        assert!(frozen.turns.iter().any(|t| t.text.contains("Sure")));
    }

    #[test]
    fn a_much_longer_late_final_lands_in_the_next_window() {
        let mut open = Vec::new();
        let mut live = LiveState::default();
        apply_event(
            &mut open,
            &mut live,
            Speaker::Me,
            "I work on pay",
            false,
            10,
        );
        let frozen = freeze_with_live(&mut open, &mut live, 1, 20);
        apply_event(
            &mut open,
            &mut live,
            Speaker::Me,
            "I work on payments at Stripe in the billing team",
            true,
            25,
        );
        assert!(frozen
            .turns
            .iter()
            .any(|t| t.text.contains("I work on pay")));
        assert_eq!(open.len(), 1, "extra words must not vanish: {open:?}");
        assert!(open[0].text.contains("Stripe"));
    }

    #[test]
    fn a_late_final_of_a_frozen_interim_is_not_duplicated_in_the_next_window() {
        let mut open = Vec::new();
        let mut live = LiveState::default();
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "Will you join within two weeks",
            false,
            10,
        );
        let frozen = freeze_with_live(&mut open, &mut live, 1, 20);
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "Will you join within two weeks?",
            true,
            25,
        );
        assert_eq!(frozen.turns.len(), 1);
        assert!(
            open.is_empty(),
            "late final must not start the next window: {open:?}"
        );
    }

    #[test]
    fn speech_after_the_cutoff_belongs_only_to_the_next_window() {
        let mut open = vec![turn(Speaker::Other, "first question")];
        let mut live = LiveState::default();
        let frozen = freeze_with_live(&mut open, &mut live, 1, 10);
        apply_event(
            &mut open,
            &mut live,
            Speaker::Me,
            "that is a new window",
            true,
            15,
        );
        assert_eq!(frozen.turns.len(), 1);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].text, "that is a new window");
        assert!(frozen
            .turns
            .iter()
            .all(|t| t.text != "that is a new window"));
    }

    #[test]
    fn a_final_restatement_replaces_the_open_interim_instead_of_duplicating() {
        let mut open = Vec::new();
        let mut live = LiveState::default();
        apply_event(&mut open, &mut live, Speaker::Me, "I mean STEM", false, 1);
        apply_event(
            &mut open,
            &mut live,
            Speaker::Me,
            "I mean STEM OPT",
            true,
            2,
        );
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].text, "I mean STEM OPT");
    }

    #[test]
    fn both_speakers_keep_independent_interims() {
        let mut open = Vec::new();
        let mut live = LiveState::default();
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "Can you start Monday",
            false,
            1,
        );
        apply_event(&mut open, &mut live, Speaker::Me, "Yes I can", false, 2);
        let frozen = freeze_with_live(&mut open, &mut live, 1, 3);
        let joined = frozen.render();
        assert!(joined.contains("Can you start Monday"), "{joined}");
        assert!(joined.contains("Yes I can"), "{joined}");
    }

    #[test]
    fn freeze_of_empty_committed_and_empty_interim_is_empty() {
        let mut open = Vec::new();
        let mut live = LiveState::default();
        let frozen = freeze_with_live(&mut open, &mut live, 1, 2);
        assert!(frozen.is_empty());
    }

    #[test]
    fn freeze_and_prompt_assembly_stay_inside_the_local_budget() {
        let started = std::time::Instant::now();
        let mut open = Vec::new();
        let mut live = LiveState::default();
        for i in 0..40 {
            apply_event(
                &mut open,
                &mut live,
                if i % 2 == 0 {
                    Speaker::Other
                } else {
                    Speaker::Me
                },
                &format!("turn {i} about the payments migration"),
                true,
                i as u64,
            );
        }
        apply_event(
            &mut open,
            &mut live,
            Speaker::Other,
            "what was your main responsibility",
            false,
            41,
        );
        let frozen = freeze_with_live(&mut open, &mut live, 1, 42);
        let freeze_ms = started.elapsed().as_millis();
        let prompt_started = std::time::Instant::now();
        let prompt = super::super::prompt::build_prompt(
            &super::super::prompt::DefaultContext {
                resume: Some("Engineer. ".repeat(200)),
                ..super::super::prompt::DefaultContext::default()
            },
            &super::super::types::SessionMemory::default(),
            &frozen,
        );
        let prompt_ms = prompt_started.elapsed().as_millis();
        assert!(frozen
            .turns
            .iter()
            .any(|t| t.text.contains("responsibility")));
        assert!(prompt.user.contains("payments migration"));
        assert!(
            freeze_ms <= 50,
            "window freeze should stay well under 50ms in debug, was {freeze_ms}ms"
        );
        assert!(
            prompt_ms <= 50,
            "warm prompt assembly should stay well under 50ms in debug, was {prompt_ms}ms"
        );
    }
}
