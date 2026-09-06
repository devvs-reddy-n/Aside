//! One current extractive topic card per session. No extra LLM on the hot path.

use rusqlite::{params, Connection};
use uuid::Uuid;

pub const CHAR_CAP: usize = 1200;

pub fn upsert(
    conn: &Connection,
    session_id: Uuid,
    text: &str,
    updated_at_ms: u64,
) -> Result<(), String> {
    let clipped = clip(text, CHAR_CAP);
    conn.execute(
        "INSERT INTO topic_summaries (session_id, text, updated_at_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET
            text = excluded.text,
            updated_at_ms = excluded.updated_at_ms",
        params![session_id.to_string(), clipped, updated_at_ms],
    )
    .map_err(|e| format!("failed to upsert topic summary: {e}"))?;
    Ok(())
}

pub fn clip(text: &str, cap: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= cap {
        return t.to_string();
    }
    t.chars().take(cap).collect()
}

/// Last 1–3 exchanges, interviewer Q + actual user answer only.
/// Never the AI suggestion. Cap ~1200 characters (~300 tokens).
pub fn extractive_from_exchanges(exchanges: &[(String, Option<String>)], cap: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (q, a) in exchanges
        .iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let mut block = format!("Q: {}", q.trim());
        if let Some(ans) = a.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            block.push_str("\nYou: ");
            block.push_str(ans);
        }
        parts.push(block);
    }
    clip(&parts.join("\n"), cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extractive_summary_uses_the_users_words_not_an_ai_stand_in() {
        let text = extractive_from_exchanges(
            &[
                ("what visa?".into(), Some("STEM OPT".into())),
                ("notice period?".into(), Some("two weeks".into())),
            ],
            CHAR_CAP,
        );
        assert!(text.contains("STEM OPT"));
        assert!(text.contains("two weeks"));
        assert!(!text.to_lowercase().contains("h-1b"));
    }

    #[test]
    fn clip_honors_the_character_cap() {
        let long = "x".repeat(2000);
        assert_eq!(clip(&long, CHAR_CAP).chars().count(), CHAR_CAP);
    }
}
