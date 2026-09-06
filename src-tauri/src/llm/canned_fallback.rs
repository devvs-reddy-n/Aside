//! The last resort: a pre-written message so the panel is never fully
//! silent when every configured LLM provider has failed.
//!
//! Credited in `status.md` 6.4 to OpenCluely, though what it actually does
//! (per this phase's research, see `llm/mod.rs`'s doc comment) is a
//! *model-name* fallback within Gemini alone, terminating in hardcoded
//! per-skill canned strings (`generateFallbackResponse`,
//! `src/services/llm.service.js`) tagged `usedFallback: true` so its own UI
//! knows not to treat it as a real model answer. This is the same idea,
//! generalized to Aside's provider-level `FallbackLlmProvider`: one plain
//! message rather than OpenCluely's several skill-specific ones, since
//! Aside has no per-skill routing to key off of at this phase.

/// The message shown when every provider in the chain failed. A constant
/// function (not a literal `const &str`) so a future version can vary it
/// (e.g. by time-of-day or by which providers were tried) without changing
/// every call site's shape.
pub fn message() -> &'static str {
    "I couldn't reach any AI provider just now, so I don't have a suggested answer for that one — \
check your API keys in settings, or just go with your own read on the question."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canned_message_is_never_empty() {
        assert!(!message().trim().is_empty());
    }

    #[test]
    fn the_canned_message_is_stable_across_calls() {
        assert_eq!(message(), message());
    }
}
