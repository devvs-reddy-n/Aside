//! The prompt every provider sends: the triggering question, plus an
//! optional retrieved-context block (`status.md` Phase 7's payoff wired in —
//! see `llm::retrieve_context`).
//!
//! Both reference apps feed a wider window (Natively: a 180-second rolling
//! transcript, `IntelligenceEngine.ts`'s `getContext(180)`; OpenCluely: the
//! last 10-15 conversation turns, `llm.service.js`'s `getConversationHistory`).
//! Aside instead feeds *retrieved* chunks (RAG top-k over the whole session,
//! not just a recent window) — closer to Natively's own `RAGRetriever.ts` +
//! `rag/prompts.ts`, which splice retrieved snippets into a single prompt
//! string via plain templating rather than a separate context/user-message
//! channel; the same plain-splice shape is used here (`context_block` below)
//! since Aside's `LlmProvider` trait only threads one final string per turn.
//! No conversation-history window is added independent of retrieval — that
//! would be Natively's rolling-window idea, not adopted, since retrieval
//! already answers "what's relevant," and a second independent window would
//! just duplicate tokens for no proven benefit at this project's scale.
//!
//! **Cleaned-question restatement (2026-08-25, owner request).** Live
//! speech is often disfluent — filler words, false starts, pauses mid-
//! sentence — so the raw transcript text is a poor thing to show back to
//! the user as "the question they were asked." Neither reference repo does
//! this (see the research note in `docs/decisions.md`); Aside asks the
//! model itself to restate the question cleanly before answering it, in
//! one streamed call rather than a second network round trip — a plain
//! `Q: <restated question>` line followed by `A: <answer>`, not JSON,
//! because a delimiter survives incremental token-by-token streaming and
//! JSON does not. `llm::question_split::QuestionAnswerSplitter` parses that
//! shape back out of the stream on the way to the frontend.
pub const SYSTEM_PROMPT: &str = "You are Aside, a quiet live-conversation copilot. The user is in a live \
conversation (an interview, a meeting). The question below may be from the other person (the interviewer) \
or from the user asking you for help — answer either. The transcript may be messy — filler words, false \
starts, dropped punctuation, mid-sentence pauses — because it comes from live speech, not writing. First, \
restate what they were actually asking as one clean, grammatical sentence, on its own line starting with \
exactly \"Q: \". Then, on a new line starting with exactly \"A: \", suggest a good answer the user could \
say out loud themselves, in first person. \
Start the A: section immediately with a FAST OPENING: 2-4 short bullets the user can say in the next few \
seconds. Then continue with the fuller answer (more detail, key steps, or code if the question needs it). \
Do not wait to plan the whole answer before writing those opening bullets. No preamble like \"Sure, here's \
an answer\" — just the restated question, then the fast opening, then the rest. \
The fact ledger in this system message is authoritative. If a resume or retrieved snippet disagrees, the ledger wins.";

/// Delimiters so providers can keep a `(question, context)` signature while
/// still putting a stable L1+L2 prefix in messages[0] and the dynamic
/// L3–L5 body in the user turn.
const STABLE_SENTINEL: &str = "\u{001e}ASIDE_STABLE\n";
const USER_SENTINEL: &str = "\n\u{001e}ASIDE_USER\n";

pub fn encode_messages(system: &str, user: &str) -> String {
    format!("{STABLE_SENTINEL}{system}{USER_SENTINEL}{user}")
}

/// Split an encoded context back into (system, user). Ordinary RAG context
/// without the sentinel keeps today's single-question user_prompt shape.
pub fn compose_system_and_user(question: &str, context: Option<&str>) -> (String, String) {
    if let Some(c) = context {
        if let Some(rest) = c.strip_prefix(STABLE_SENTINEL) {
            if let Some((system, user)) = rest.split_once(USER_SENTINEL) {
                return (system.to_string(), user.to_string());
            }
        }
    }
    (SYSTEM_PROMPT.to_string(), user_prompt(question, context))
}

/// A short, stable identifier for a prompt prefix, sent as OpenAI's
/// `prompt_cache_key` so its automatic prefix caching can recognize that
/// every question in a session repeats the same system prompt.
///
/// FNV-1a rather than a real cryptographic hash, and deliberately so: the
/// value only needs to be *stable* and collision-tolerant, not secure.
/// Pulling in `sha2` for fifteen lines of arithmetic would be a dependency
/// with no benefit.
///
/// Honest caveat, also recorded at the call site: OpenAI's caching only
/// engages above a substantial identical prefix length, so on Aside's short
/// system prompt this likely does nothing until the conversation block
/// grows. It costs one hash per request, so it's sent anyway.
pub fn cache_key(seed: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in seed.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("aside-{hash:016x}")
}

/// The user-turn content built from the triggering question, optionally
/// preceded by a retrieved-context block. `context` is `None`/empty for a
/// session with no `session_id`, no matching chunks yet, or a failed
/// embedder — the prompt degrades to exactly today's single-question shape
/// rather than mentioning a context block that isn't there.
pub fn user_prompt(question: &str, context: Option<&str>) -> String {
    let context_block = match context {
        Some(c) if !c.trim().is_empty() => {
            format!(
                "Relevant context from earlier in this conversation:\n\n{}\n\n---\n\n",
                c.trim()
            )
        }
        _ => String::new(),
    };
    format!(
        "{context_block}The question to help with:\n\n\"{}\"\n\nSuggest a good, concise answer. Start with a fast 2-4 bullet opening, then continue.",
        question.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_prompt_includes_the_question_verbatim() {
        let p = user_prompt("What's your biggest weakness?", None);
        assert!(p.contains("What's your biggest weakness?"));
    }

    #[test]
    fn user_prompt_trims_the_question() {
        let p = user_prompt("  how are you  ", None);
        assert!(p.contains("\"how are you\""));
        assert!(!p.contains("  how are you  "));
    }

    #[test]
    fn system_prompt_is_non_empty_and_stable() {
        assert!(!SYSTEM_PROMPT.is_empty());
        assert!(SYSTEM_PROMPT.contains("FAST OPENING"));
        assert!(SYSTEM_PROMPT.contains("the user asking you for help"));
    }

    #[test]
    fn no_context_produces_exactly_the_prior_single_question_shape() {
        let with_none = user_prompt("what's your notice period", None);
        let with_empty = user_prompt("what's your notice period", Some("   "));
        assert!(!with_none.contains("Relevant context"));
        assert_eq!(
            with_none, with_empty,
            "blank/whitespace-only context must degrade the same as None"
        );
    }

    #[test]
    fn real_context_appears_before_the_question_block() {
        let p = user_prompt(
            "what's my notice period again?",
            Some("[you] my notice period is two weeks"),
        );
        assert!(p.contains("Relevant context from earlier in this conversation"));
        assert!(p.contains("[you] my notice period is two weeks"));
        let context_pos = p.find("Relevant context").unwrap();
        let question_pos = p.find("The question to help with").unwrap();
        assert!(
            context_pos < question_pos,
            "context must come before the question block"
        );
    }

    #[test]
    fn cache_key_is_stable_for_the_same_seed() {
        // The whole point: the same prompt must produce the same key on
        // every request, or the provider's cache never hits.
        assert_eq!(cache_key(SYSTEM_PROMPT), cache_key(SYSTEM_PROMPT));
    }

    #[test]
    fn cache_key_differs_for_different_seeds() {
        assert_ne!(cache_key("one"), cache_key("two"));
    }

    #[test]
    fn cache_key_is_short_and_ascii() {
        // Provider-side keys have length limits; keep it comfortably small
        // and free of anything that would need escaping.
        let k = cache_key(SYSTEM_PROMPT);
        assert!(k.len() <= 32, "got {} chars: {k}", k.len());
        assert!(
            k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "got {k}"
        );
    }

    #[test]
    fn context_is_trimmed() {
        let p = user_prompt("q", Some("  padded context  "));
        assert!(p.contains("\npadded context\n"));
        assert!(!p.contains("  padded context  "));
    }

    #[test]
    fn encoded_messages_round_trip_without_leaking_sentinels_into_the_model_text() {
        let encoded = encode_messages("SYS", "USER-Q");
        let (system, user) = compose_system_and_user("ignored", Some(&encoded));
        assert_eq!(system, "SYS");
        assert_eq!(user, "USER-Q");
        assert!(!system.contains('\u{001e}'));
        assert!(!user.contains('\u{001e}'));
    }

    #[test]
    fn prefix_stability_identical_messages_zero_across_questions() {
        let sys = format!("{SYSTEM_PROMPT}\n\nFact ledger:\n- visa_status: STEM OPT");
        let a = compose_system_and_user("q1", Some(&encode_messages(&sys, "user one")));
        let b = compose_system_and_user("q2", Some(&encode_messages(&sys, "user two")));
        assert_eq!(a.0, b.0);
        assert_ne!(a.1, b.1);
        assert_eq!(cache_key(&a.0), cache_key(&b.0));
    }
}
