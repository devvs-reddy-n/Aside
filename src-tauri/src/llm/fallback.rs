//! `FallbackLlmProvider` — tries providers in a fixed order, falling back to
//! [`canned_fallback`] if every one fails. `status.md` 6.4's "OpenCluely's
//! best idea", generalized from a model-name list to a provider list — see
//! `llm/mod.rs`'s doc comment for the full research finding.
//!
//! Split into two pieces on purpose:
//! - [`FallbackChain`] — pure bookkeeping (the order, and a per-provider
//!   failure counter). No network, no `AppHandle`, no async — fully
//!   unit-testable, per `status.md` 6.7's explicit call-out that this logic
//!   is "pure and unit-testable" without a real key.
//! - [`FallbackLlmProvider`] — the real async orchestration: owns the
//!   actual `Box<dyn LlmProvider>` instances and a `FallbackChain`, and
//!   does the emit-events-as-it-goes streaming work `mod.rs`'s
//!   `respond_to_question` calls.

use std::collections::HashMap;
use std::sync::Mutex;

use super::{canned_fallback, AnswerSink, LlmProvider, LlmProviderKind, LlmRuntime};

/// Pure ordering + failure-count bookkeeping. The order is fixed at
/// construction (the user's configured primary provider first, then the
/// rest in `LlmProviderKind::ALL`'s canonical order — see
/// `build_provider_chain`) — a failed provider is *not* reordered or
/// demoted within a chain's lifetime, only counted. Reordering on failure
/// was considered and rejected: a transient failure (one rate-limited
/// request) shouldn't permanently demote an otherwise-preferred provider,
/// and Natively's own richer `ProviderRouter`/`CircuitBreaker` — which does
/// something like that — turned out to not even be wired into production
/// (see `mod.rs`'s doc comment), which is evidence this simpler shape is
/// the right amount of complexity for Phase 6.
#[derive(Debug, Default)]
pub struct FallbackChain {
    order: Vec<LlmProviderKind>,
    failure_counts: HashMap<LlmProviderKind, u32>,
}

impl FallbackChain {
    pub fn new(order: Vec<LlmProviderKind>) -> Self {
        Self {
            order,
            failure_counts: HashMap::new(),
        }
    }

    /// `#[allow(dead_code)]`: real inspection API (proven by this file's own
    /// `FallbackChain` tests, which are only compiled under `cargo test`,
    /// not the plain `cargo build --lib` the dead-code lint runs under) for
    /// a future settings/debug UI that might want to show which provider
    /// order is configured — not scaffolding, matches
    /// `storage/transcripts.rs`'s already-stated precedent for this exact
    /// situation.
    #[allow(dead_code)]
    pub fn order(&self) -> &[LlmProviderKind] {
        &self.order
    }

    pub fn record_failure(&mut self, kind: LlmProviderKind) {
        *self.failure_counts.entry(kind).or_insert(0) += 1;
    }

    /// `#[allow(dead_code)]` — see `order`'s doc comment above; same
    /// situation, a future consumer (e.g. surfacing "OpenAI has failed 3
    /// times this session" in a settings screen) rather than dead
    /// scaffolding.
    #[allow(dead_code)]
    pub fn failure_count(&self, kind: LlmProviderKind) -> u32 {
        *self.failure_counts.get(&kind).unwrap_or(&0)
    }
}

/// The real async orchestrator. `providers` and `chain.order()` are kept in
/// sync by construction (`FallbackLlmProvider::new` builds `chain` from the
/// same `order` it's given) — `stream_answer` walks `providers` in order,
/// which is walking the chain's order, without needing to look each one up
/// by kind on every call.
pub struct FallbackLlmProvider {
    providers: Vec<Box<dyn LlmProvider>>,
    chain: Mutex<FallbackChain>,
}

impl FallbackLlmProvider {
    pub fn new(order: Vec<LlmProviderKind>, providers: Vec<Box<dyn LlmProvider>>) -> Self {
        Self {
            chain: Mutex::new(FallbackChain::new(order)),
            providers,
        }
    }

    /// The order actually being tried — matches `providers` exactly (both
    /// are filtered to keyed providers by `mod.rs::build_provider_chain`
    /// *before* calling `new`, in lockstep). Exposed for tests/inspection —
    /// see `FallbackChain::order`'s doc comment for why `#[allow(dead_code)]`
    /// is honest here, not a cover for scaffolding.
    #[allow(dead_code)]
    pub fn order(&self) -> Vec<LlmProviderKind> {
        self.chain
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .order()
            .to_vec()
    }

    #[allow(dead_code)] // test/inspection API — see `order`'s doc comment above
    pub fn providers_is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// The model the *first* provider in the chain will use — the one that
    /// answers unless it fails. Reported in the timing readout and shown
    /// read-only in the panel so "which model is answering" is visible
    /// mid-session without a second editable surface.
    pub fn active_model(&self) -> Option<String> {
        self.providers.first().and_then(|p| p.model_name())
    }

    /// Warms the answering provider's prompt cache (the first in the chain).
    pub async fn prewarm(&self, system: &str) -> Result<super::timing::TokenUsage, String> {
        match self.providers.first() {
            Some(provider) => provider.prewarm(system).await,
            None => Err("no AI key saved".to_string()),
        }
    }

    /// Screenshot answers use the call's own provider and model only (the
    /// first in the chain). No cross-provider retry: a picture is large, and
    /// a silent switch to another model would surprise more than help.
    pub async fn stream_vision(
        &self,
        system: &str,
        user_text: &str,
        jpeg_base64: &str,
        sink: &mut dyn AnswerSink,
    ) -> Result<(), String> {
        let provider = self
            .providers
            .first()
            .ok_or_else(|| "No AI key saved yet — open Settings and add one.".to_string())?;
        provider.stream_vision(system, user_text, jpeg_base64, sink).await
    }

    #[allow(dead_code)] // test/inspection API — see `order`'s doc comment above
    pub fn failure_count(&self, kind: LlmProviderKind) -> u32 {
        self.chain
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .failure_count(kind)
    }

    /// Tries every provider in order; the first to succeed ends the call
    /// (having already streamed its own deltas through `sink`, this
    /// function then reports the terminal `on_done(false)`). If every
    /// provider fails, sends `canned_fallback::message()` as a single delta
    /// plus `on_done(true)`, so the panel is never left blank. Each
    /// per-provider failure is reported via `sink.on_error` before moving
    /// on — diagnostic, not terminal.
    #[cfg(test)]
    pub async fn stream_answer(
        &self,
        question: &str,
        context: Option<&str>,
        sink: &mut dyn AnswerSink,
    ) {
        self.stream_with_gate(question, context, sink, None).await;
    }

    pub async fn stream_with_gate(
        &self,
        question: &str,
        context: Option<&str>,
        sink: &mut dyn AnswerSink,
        gate: Option<&LlmRuntime>,
    ) {
        for provider in &self.providers {
            let kind = provider.kind();
            if gate.map(|g| g.is_open(kind)).unwrap_or(false) {
                sink.on_error(&format!("{kind:?} skipped after repeated failures"));
                continue;
            }
            // Each attempt is about to send its own, independent token
            // stream — `AnswerSink::on_attempt_start`'s doc comment has the
            // full story on why a stateful sink (`SplittingSink`) needs this
            // signal between attempts, not just once at the top of this call.
            sink.on_attempt_start();
            match provider.stream_answer(question, context, sink).await {
                Ok(()) => {
                    if let Some(gate) = gate {
                        gate.record_success(kind);
                    }
                    sink.on_done(false);
                    return;
                }
                Err(e) => {
                    self.chain
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .record_failure(kind);
                    if let Some(gate) = gate {
                        gate.record_failure(kind);
                    }
                    sink.on_error(&format!("{kind:?} failed: {e}"));
                }
            }
        }
        sink.on_attempt_start();
        sink.on_delta(canned_fallback::message());
        sink.on_done(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freshly_built_chain_has_no_failures_recorded() {
        let chain = FallbackChain::new(vec![LlmProviderKind::OpenAi, LlmProviderKind::Anthropic]);
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 0);
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 0);
    }

    #[test]
    fn recording_a_failure_increments_only_that_providers_counter() {
        let mut chain = FallbackChain::new(vec![LlmProviderKind::OpenAi, LlmProviderKind::Anthropic]);
        chain.record_failure(LlmProviderKind::OpenAi);
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 1);
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 0);
    }

    #[test]
    fn repeated_failures_accumulate() {
        let mut chain = FallbackChain::new(vec![LlmProviderKind::Anthropic]);
        for _ in 0..5 {
            chain.record_failure(LlmProviderKind::Anthropic);
        }
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 5);
    }

    #[test]
    fn order_is_preserved_exactly_as_constructed() {
        let order = vec![LlmProviderKind::Anthropic, LlmProviderKind::OpenAi];
        let chain = FallbackChain::new(order.clone());
        assert_eq!(chain.order(), order.as_slice());
    }

    #[test]
    fn a_provider_never_tried_reports_zero_failures_not_an_error() {
        let chain = FallbackChain::new(vec![LlmProviderKind::OpenAi]);
        // Anthropic isn't even in the order, but querying its count must not panic.
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 0);
    }

    // --- FallbackLlmProvider: fake providers + an in-memory sink, so the
    // full orchestration (try in order, record failures, stop at the first
    // success, fall back to canned when everything fails) is exercised
    // without any real network call or running Tauri app — exactly the
    // decoupling `AnswerSink` exists for (see `mod.rs`'s doc comment on the
    // trait). Real per-provider request/stream parsing is tested in
    // openai.rs and anthropic.rs.

    struct FakeProvider {
        kind: LlmProviderKind,
        outcome: Result<&'static str, &'static str>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for FakeProvider {
        fn kind(&self) -> LlmProviderKind {
            self.kind
        }
        async fn stream_answer(
            &self,
            _question: &str,
            _context: Option<&str>,
            sink: &mut dyn AnswerSink,
        ) -> Result<(), String> {
            match self.outcome {
                Ok(text) => {
                    sink.on_delta(text);
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            }
        }
    }

    #[derive(Default)]
    struct CollectingSink {
        deltas: Vec<String>,
        done: Option<bool>,
        errors: Vec<String>,
        attempt_starts: u32,
    }

    impl AnswerSink for CollectingSink {
        fn on_delta(&mut self, text: &str) {
            self.deltas.push(text.to_string());
        }
        fn on_done(&mut self, canned: bool) {
            self.done = Some(canned);
        }
        fn on_error(&mut self, message: &str) {
            self.errors.push(message.to_string());
        }
        fn on_attempt_start(&mut self) {
            self.attempt_starts += 1;
        }
    }

    fn chain_of(providers: Vec<FakeProvider>) -> FallbackLlmProvider {
        let order = providers.iter().map(|p| p.kind).collect();
        let boxed: Vec<Box<dyn LlmProvider>> = providers
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn LlmProvider>)
            .collect();
        FallbackLlmProvider::new(order, boxed)
    }

    #[tokio::test]
    async fn the_first_provider_to_succeed_ends_the_chain_without_trying_the_rest() {
        let chain = chain_of(vec![
            FakeProvider {
                kind: LlmProviderKind::OpenAi,
                outcome: Ok("hello from openai"),
            },
            FakeProvider {
                kind: LlmProviderKind::Anthropic,
                outcome: Ok("should never be reached"),
            },
        ]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("what's up", None, &mut sink).await;

        assert_eq!(sink.deltas, vec!["hello from openai"]);
        assert_eq!(sink.done, Some(false));
        assert!(sink.errors.is_empty());
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 0);
        assert_eq!(
            sink.attempt_starts, 1,
            "one provider tried == one attempt-start signal"
        );
    }

    #[tokio::test]
    async fn on_attempt_start_fires_once_per_provider_tried_plus_once_for_the_canned_fallback() {
        // Regression test for the cross-attempt splitter-contamination bug
        // an adversarial review caught: `AnswerSink::on_attempt_start` must
        // fire before *every* fresh token source this orchestration hands
        // the sink, including the terminal canned message — not just once
        // at the very start of the call — so a stateful sink like
        // `SplittingSink` can reset between attempts.
        let chain = chain_of(vec![
            FakeProvider {
                kind: LlmProviderKind::OpenAi,
                outcome: Err("boom"),
            },
            FakeProvider {
                kind: LlmProviderKind::Anthropic,
                outcome: Err("boom too"),
            },
        ]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("q", None, &mut sink).await;

        // 2 failed attempts + 1 for the canned-fallback delta that follows.
        assert_eq!(sink.attempt_starts, 3);
        assert_eq!(sink.done, Some(true));
    }

    #[tokio::test]
    async fn a_failing_primary_falls_through_to_the_next_provider_in_order() {
        let chain = chain_of(vec![
            FakeProvider {
                kind: LlmProviderKind::OpenAi,
                outcome: Err("401 unauthorized"),
            },
            FakeProvider {
                kind: LlmProviderKind::Anthropic,
                outcome: Ok("hello from anthropic"),
            },
        ]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("what's up", None, &mut sink).await;

        assert_eq!(sink.deltas, vec!["hello from anthropic"]);
        assert_eq!(sink.done, Some(false));
        assert_eq!(
            sink.errors.len(),
            1,
            "the openai failure should be reported once"
        );
        assert!(sink.errors[0].contains("OpenAi"));
        assert!(sink.errors[0].contains("401 unauthorized"));
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 1);
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 0);
    }

    #[tokio::test]
    async fn every_provider_failing_falls_back_to_the_canned_message() {
        let chain = chain_of(vec![
            FakeProvider {
                kind: LlmProviderKind::OpenAi,
                outcome: Err("network error"),
            },
            FakeProvider {
                kind: LlmProviderKind::Anthropic,
                outcome: Err("rate limited"),
            },
        ]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("what's up", None, &mut sink).await;

        assert_eq!(sink.deltas, vec![canned_fallback::message().to_string()]);
        assert_eq!(
            sink.done,
            Some(true),
            "canned fallback must report done(canned: true)"
        );
        assert_eq!(
            sink.errors.len(),
            2,
            "both provider failures should be reported"
        );
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 1);
        assert_eq!(chain.failure_count(LlmProviderKind::Anthropic), 1);
    }

    #[tokio::test]
    async fn an_empty_provider_list_goes_straight_to_the_canned_message() {
        // The real case `mod.rs`'s `build_provider_chain` produces when no
        // API key is saved for any provider at all.
        let chain = chain_of(vec![]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("what's up", None, &mut sink).await;

        assert_eq!(sink.deltas, vec![canned_fallback::message().to_string()]);
        assert_eq!(sink.done, Some(true));
        assert!(sink.errors.is_empty(), "there was nothing to fail");
    }

    #[tokio::test]
    async fn providers_are_tried_strictly_in_the_configured_order() {
        let chain = chain_of(vec![
            FakeProvider {
                kind: LlmProviderKind::Anthropic,
                outcome: Err("anthropic down"),
            },
            FakeProvider {
                kind: LlmProviderKind::OpenAi,
                outcome: Ok("openai answered"),
            },
        ]);
        let mut sink = CollectingSink::default();
        chain.stream_answer("what's up", None, &mut sink).await;

        assert_eq!(sink.deltas, vec!["openai answered"]);
        // Claude first because the call chose Claude, matching the
        // constructed order exactly.
        assert!(sink.errors[0].contains("Anthropic"));
        assert_eq!(chain.failure_count(LlmProviderKind::OpenAi), 0);
    }

    #[tokio::test]
    async fn repeated_failures_skip_the_provider_on_the_next_press() {
        let chain = chain_of(vec![FakeProvider {
            kind: LlmProviderKind::OpenAi,
            outcome: Err("boom"),
        }]);
        let gate = crate::llm::LlmRuntime::default();
        for _ in 0..crate::llm::CIRCUIT_OPEN_AFTER {
            let mut sink = CollectingSink::default();
            chain
                .stream_with_gate("q", None, &mut sink, Some(&gate))
                .await;
            assert_eq!(sink.done, Some(true));
        }
        assert!(gate.is_open(LlmProviderKind::OpenAi));
        let mut sink = CollectingSink::default();
        chain
            .stream_with_gate("q", None, &mut sink, Some(&gate))
            .await;
        assert!(
            sink.errors.iter().any(|e| e.contains("skipped")),
            "open circuit must skip the network: {:?}",
            sink.errors
        );
        assert_eq!(sink.done, Some(true));
        assert_eq!(sink.deltas, vec![canned_fallback::message().to_string()]);
    }

    #[tokio::test]
    async fn changing_settings_closes_an_open_circuit() {
        let gate = crate::llm::LlmRuntime::default();
        for _ in 0..crate::llm::CIRCUIT_OPEN_AFTER {
            gate.record_failure(LlmProviderKind::OpenAi);
        }
        assert!(gate.is_open(LlmProviderKind::OpenAi));
        gate.invalidate_settings();
        assert!(!gate.is_open(LlmProviderKind::OpenAi));
    }
}
