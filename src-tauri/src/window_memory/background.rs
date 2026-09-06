//! Async, single-consumer background memory pipeline (owner's single
//! growing-summary spec, 2026-08-29 — simplifies the earlier two-layer
//! recent/historical design: no rollover, no re-consolidation, just append).
//!
//! **Correctness, not just cost.** A completed conversation window is fed
//! to a cheap second model (the real implementation lives in
//! `llm::background::LlmCompressor`) that is given *only* the raw completed
//! transcript window — `Other:`/`Me:` lines — and never the visible answer
//! the main model streamed to the user. Because the compressor structurally
//! cannot see the AI's own suggested answer, it cannot leak an unspoken
//! suggestion back into memory as if a real participant had said it. That
//! is the same "spoken truth beats an AI suggestion" rule the old
//! `merge::leaks_prior_suggestion` filter used to enforce after the fact —
//! this design enforces it by construction instead, which is why that
//! filter (and the whole `ContextUpdate` shape it protected) is gone.
//!
//! **Never blocks the visible answer.** `enqueue_window` is a non-blocking
//! channel send, called right after a window is frozen — not awaited
//! before the answer stream starts, and not awaited after it finishes
//! either (`llm::respond_to_window`).
//!
//! **Ordering, the simple way.** Every window for the one running app goes
//! through a single `mpsc` queue with exactly one consumer
//! ([`MemoryWorker`]). The consumer always awaits one job to full
//! completion before pulling the next one off the queue, so window N's
//! merge finishes before window N+1's starts no matter which network call
//! happens to answer first — no version numbers or CAS needed. See
//! `two_windows_finish_out_of_order_but_summary_still_reflects_enqueue_order`
//! below for the regression test proving it.
//!
//! **Failure handling.** [`run_job`] only ever commits once its network call
//! has already succeeded (see its own doc comment) — a failure leaves
//! `summary` completely untouched, is logged, and never blocks or crashes
//! anything else.

use tauri::{AppHandle, Emitter, Manager};

use super::merge;
use super::types::{ConversationWindow, SessionMemory, WindowPreview};
use super::{WindowMemory, EVENT_WINDOW_MEMORY};

/// What the background pipeline needs from a model: compress one completed
/// window into a short prose summary. The real implementation
/// (`llm::background::LlmCompressor`) is deliberately kept out of this
/// module — `window_memory` stays free of any actual network code, matching
/// the rest of this module's "pure core" convention — and a fake
/// implementation is what makes [`run_job`] and [`MemoryWorker`] testable
/// without a running Tauri app or a real network call.
#[async_trait::async_trait]
pub trait Compressor: Send + Sync {
    /// Compress one completed window's raw transcript (`Other:`/`Me:`
    /// lines) into a 1-2 sentence prose summary. Must never be given, and
    /// must never need, the visible answer the main model suggested.
    async fn summarize_window(&self, window_text: &str) -> Result<String, String>;
}

/// Where [`run_job`] reads and writes memory. A thin seam: the real
/// implementation ([`AppMemoryStore`]) locks the actual Tauri-managed
/// [`WindowMemory`] state and emits the `window-memory` event on commit; a
/// plain `std::sync::Mutex<SessionMemory>` (see the tests below) is enough
/// for everything that doesn't need a running app.
pub trait MemoryStore: Send + Sync {
    fn snapshot(&self) -> SessionMemory;
    fn commit(&self, summary: Vec<String>);
}

impl MemoryStore for std::sync::Mutex<SessionMemory> {
    fn snapshot(&self) -> SessionMemory {
        self.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn commit(&self, summary: Vec<String>) {
        let mut guard = self.lock().unwrap_or_else(|p| p.into_inner());
        guard.summary = summary;
    }
}

/// Runs one window's background summarization to completion: summarize,
/// then append to `summary`.
///
/// **All-or-nothing.** The one fallible step (`?`) happens *before*
/// `store.commit` is ever called — so a `summarize_window` failure never
/// touches memory at all. This is what point 6 of the owner's original spec
/// ("leave existing memory untouched" on failure) means in practice — see
/// `a_failed_summary_call_leaves_memory_completely_untouched`.
/// Returns the new line actually appended (`Ok(Some(prose))`), so a caller
/// with a real session id (`RealRunner`) can persist that one line to
/// `storage::window_summaries` — a past session's transcript view needs
/// this list to survive after the session ends, which the in-memory
/// `summary` this function also updates never does on its own. `Ok(None)`
/// covers both no-op cases (an empty window, an empty compressor reply) —
/// deliberately the same as before rather than a new error, since neither
/// is a real failure.
pub async fn run_job(
    compressor: &dyn Compressor,
    store: &dyn MemoryStore,
    window: &ConversationWindow,
) -> Result<Option<String>, String> {
    let window_text = window.render();
    if window_text.trim().is_empty() {
        return Ok(None);
    }

    let prose = compressor
        .summarize_window(&window_text)
        .await
        .map_err(|e| format!("window summary failed: {e}"))?;
    let prose = prose.trim().to_string();
    if prose.is_empty() {
        return Ok(None);
    }

    let mut memory = store.snapshot();
    merge::push_summary(&mut memory.summary, prose.clone());
    store.commit(memory.summary);
    Ok(Some(prose))
}

/// One consumer's job: process a fully-frozen [`ConversationWindow`]
/// against whatever [`MemoryStore`]/[`Compressor`] it was built with.
/// [`MemoryWorker`] drives exactly one of these at a time, in enqueue
/// order.
#[async_trait::async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(&self, session_id: Option<uuid::Uuid>, window: ConversationWindow);
}

/// The single-consumer sequential queue described in this module's doc
/// comment. Nothing about this type is Tauri-specific — [`MemoryWorker`]
/// only needs a background-friendly `spawn` (here `tauri::async_runtime`,
/// matching every other background task in this codebase, e.g.
/// `llm::spawn_prewarm`), so it's exercised directly (no `AppHandle`) in
/// this module's own tests.
pub struct MemoryWorker {
    tx: tokio::sync::mpsc::UnboundedSender<(Option<uuid::Uuid>, ConversationWindow)>,
}

impl MemoryWorker {
    pub fn spawn(runner: std::sync::Arc<dyn JobRunner>) -> Self {
        let (tx, mut rx) =
            tokio::sync::mpsc::unbounded_channel::<(Option<uuid::Uuid>, ConversationWindow)>();
        tauri::async_runtime::spawn(async move {
            // The `while let` only advances once `runner.run` resolves —
            // this single await point, run one job at a time, is the whole
            // ordering guarantee. No generation counters, no CAS.
            while let Some((session_id, window)) = rx.recv().await {
                runner.run(session_id, window).await;
            }
        });
        Self { tx }
    }

    /// Never blocks: an unbounded channel send returns immediately whether
    /// or not the consumer is mid-job.
    pub fn enqueue(&self, session_id: Option<uuid::Uuid>, window: ConversationWindow) {
        let _ = self.tx.send((session_id, window));
    }
}

// --- Real (Tauri-backed) wiring ---

/// [`MemoryStore`] over the real, Tauri-managed [`WindowMemory`] state.
/// Each method locks just long enough to read or write `Inner::memory` —
/// never across an `.await` — then emits the same `window-memory` event
/// every other memory-changing path in this module already fires, so the
/// panel refreshes the moment a background summary actually lands.
struct AppMemoryStore<'a> {
    app: &'a AppHandle,
}

impl<'a> MemoryStore for AppMemoryStore<'a> {
    fn snapshot(&self) -> SessionMemory {
        let state = self.app.state::<WindowMemory>();
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        guard.memory.clone()
    }

    fn commit(&self, summary: Vec<String>) {
        let state = self.app.state::<WindowMemory>();
        let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        guard.memory.summary = summary;
        let preview = WindowPreview {
            raw_window: String::new(),
            facts: guard.memory.facts.clone(),
            summary: guard.memory.summary.clone(),
            press_count: guard.press_count,
            last_log: guard.last_log.clone(),
        };
        drop(guard);
        let _ = self.app.emit(EVENT_WINDOW_MEMORY, preview);
    }
}

/// Real [`JobRunner`]: builds a fresh [`llm::background::LlmCompressor`]
/// and [`AppMemoryStore`] per job (so a mid-session provider/model/key
/// change is picked up on the very next window, not just the next app
/// launch), runs it, and logs — never panics or propagates — on failure.
struct RealRunner {
    app: AppHandle,
}

#[async_trait::async_trait]
impl JobRunner for RealRunner {
    async fn run(&self, session_id: Option<uuid::Uuid>, window: ConversationWindow) {
        let compressor = crate::llm::background::LlmCompressor::new(&self.app);
        let store = AppMemoryStore { app: &self.app };
        match run_job(&compressor, &store, &window).await {
            Ok(Some(prose)) => {
                // The in-memory `summary` list `run_job` just updated is
                // wiped the moment this session ends (`active_session::
                // clear`) — this is the durable copy, so a past session's
                // transcript view can still show it afterward. Best-effort:
                // a save failure here must never be treated as the window's
                // summarization itself having failed.
                if let Some(sid) = session_id {
                    let db = self.app.state::<crate::storage::Db>();
                    let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
                    if let Err(e) = crate::storage::window_summaries::append(
                        &conn,
                        sid,
                        &prose,
                        super::now_ms(),
                    ) {
                        eprintln!("window summary generated but not saved for history: {e}");
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("window memory background summarization skipped this window: {e}");
            }
        }
    }
}

/// Spawns the one background consumer this running app uses and manages it
/// as Tauri state. Called once, from `lib.rs`'s `setup`, the same place
/// `llm::spawn_prewarm` is called from.
pub fn init_background_worker(app: &AppHandle) {
    let runner: std::sync::Arc<dyn JobRunner> =
        std::sync::Arc::new(RealRunner { app: app.clone() });
    app.manage(MemoryWorker::spawn(runner));
}

/// Queues one frozen window for background summarization. Called right
/// after `window_memory::freeze()` returns (`llm::respond_to_window`) —
/// before the visible answer even starts streaming, and never awaited.
/// Silently drops the window (with a log line) if the worker was never
/// initialized, e.g. in a test harness with no `AppHandle` setup — the
/// visible answer path must never depend on this succeeding.
pub fn enqueue_window(app: &AppHandle, session_id: Option<uuid::Uuid>, window: ConversationWindow) {
    match app.try_state::<MemoryWorker>() {
        Some(worker) => worker.enqueue(session_id, window),
        None => eprintln!(
            "window memory background worker not initialized; one window's summary was dropped"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_memory::types::{RawTurn, Speaker};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    fn turn(speaker: Speaker, text: &str) -> RawTurn {
        RawTurn {
            speaker,
            text: text.to_string(),
            at_ms: 0,
        }
    }

    fn window(lines: &[(Speaker, &str)]) -> ConversationWindow {
        ConversationWindow {
            turns: lines.iter().map(|(s, t)| turn(*s, t)).collect(),
            start_ms: 0,
            end_ms: 1,
        }
    }

    /// A controllable fake: sleeps `delay_ms` on `summarize_window` (so
    /// tests can force out-of-order network completion), and can be told to
    /// fail.
    struct FakeCompressor {
        delay_ms: u64,
        fail_summarize: bool,
    }

    impl Default for FakeCompressor {
        fn default() -> Self {
            Self {
                delay_ms: 0,
                fail_summarize: false,
            }
        }
    }

    #[async_trait::async_trait]
    impl Compressor for FakeCompressor {
        async fn summarize_window(&self, window_text: &str) -> Result<String, String> {
            if self.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            }
            if self.fail_summarize {
                return Err("summarize boom".to_string());
            }
            Ok(format!("summary of [{window_text}]"))
        }
    }

    #[tokio::test]
    async fn run_job_appends_a_window_summary_to_summary() {
        let store: Mutex<SessionMemory> = Mutex::new(SessionMemory::default());
        let compressor = FakeCompressor::default();
        let w = window(&[
            (Speaker::Other, "How are you?"),
            (Speaker::Me, "Good, thanks."),
        ]);
        run_job(&compressor, &store, &w)
            .await
            .expect("job should succeed");
        let memory = store.snapshot();
        assert_eq!(memory.summary.len(), 1);
        assert!(memory.summary[0].contains("Other: How are you?"));
    }

    #[tokio::test]
    async fn run_job_keeps_appending_with_no_cap() {
        let mut seeded = SessionMemory::default();
        for i in 0..25 {
            seeded.summary.push(format!("old-window-{i}"));
        }
        let store: Mutex<SessionMemory> = Mutex::new(seeded);
        let compressor = FakeCompressor::default();
        let w = window(&[(Speaker::Other, "One more question?")]);

        run_job(&compressor, &store, &w)
            .await
            .expect("job should succeed");

        let memory = store.snapshot();
        assert_eq!(
            memory.summary.len(),
            26,
            "every prior window's summary must survive — no cap, no eviction"
        );
        assert_eq!(
            memory.summary[0], "old-window-0",
            "the oldest entry must not roll out"
        );
        assert!(memory
            .summary
            .last()
            .unwrap()
            .contains("One more question?"));
    }

    #[tokio::test]
    async fn a_failed_summary_call_leaves_memory_completely_untouched() {
        let seeded = SessionMemory {
            facts: vec!["f1".to_string()],
            summary: vec!["existing".to_string()],
        };
        let store: Mutex<SessionMemory> = Mutex::new(seeded.clone());
        let compressor = FakeCompressor {
            fail_summarize: true,
            ..FakeCompressor::default()
        };
        let w = window(&[(Speaker::Other, "Hello?")]);

        let err = run_job(&compressor, &store, &w)
            .await
            .expect_err("summary failure must surface as Err");
        assert!(err.contains("summarize boom"));
        assert_eq!(
            store.snapshot(),
            seeded,
            "memory must be byte-for-byte unchanged on failure"
        );
    }

    #[tokio::test]
    async fn compressor_only_ever_receives_the_raw_window_text_never_an_answer() {
        // Structural proof, not just a convention: `Compressor::summarize_window`
        // takes exactly one string, and `run_job` builds it from
        // `window.render()` alone — there is no answer text anywhere in
        // scope to pass even by mistake.
        struct CapturingCompressor(Mutex<Option<String>>);
        #[async_trait::async_trait]
        impl Compressor for CapturingCompressor {
            async fn summarize_window(&self, window_text: &str) -> Result<String, String> {
                *self.0.lock().unwrap() = Some(window_text.to_string());
                Ok("summary".to_string())
            }
        }
        let compressor = CapturingCompressor(Mutex::new(None));
        let store: Mutex<SessionMemory> = Mutex::new(SessionMemory::default());
        let w = window(&[
            (Speaker::Other, "What did you use for monitoring?"),
            (Speaker::Me, "CloudWatch."),
        ]);
        run_job(&compressor, &store, &w).await.unwrap();
        let captured = compressor.0.lock().unwrap().clone().unwrap();
        assert_eq!(
            captured,
            "Other: What did you use for monitoring?\nMe: CloudWatch."
        );
        assert!(
            !captured.to_lowercase().contains("grafana"),
            "no AI suggestion text exists to leak in the first place"
        );
    }

    struct RecordingRunner {
        store: Arc<Mutex<SessionMemory>>,
        completion_order: Arc<Mutex<Vec<String>>>,
        done: Arc<tokio::sync::Notify>,
        remaining: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl JobRunner for RecordingRunner {
        async fn run(&self, _session_id: Option<uuid::Uuid>, window: ConversationWindow) {
            let window_text = window.render();
            // window-1's summary artificially takes far longer than
            // window-2's — a real fake/mock provider with a controllable
            // delay, exactly what point 5 of the spec asks the test to use.
            let delay_ms = if window_text.contains("SLOW") { 60 } else { 5 };
            let compressor = FakeCompressor {
                delay_ms,
                ..FakeCompressor::default()
            };
            run_job(&compressor, self.store.as_ref(), &window)
                .await
                .unwrap();
            self.completion_order.lock().unwrap().push(window_text);
            if self.remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.done.notify_waiters();
            }
        }
    }

    #[tokio::test]
    async fn two_windows_finish_out_of_order_but_summary_still_reflects_enqueue_order() {
        let store = Arc::new(Mutex::new(SessionMemory::default()));
        let completion_order = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(tokio::sync::Notify::new());
        let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(2));
        let runner = Arc::new(RecordingRunner {
            store: store.clone(),
            completion_order: completion_order.clone(),
            done: done.clone(),
            remaining: remaining.clone(),
        });
        let worker = MemoryWorker::spawn(runner);

        let notified = done.notified();
        // Window 1 is tagged SLOW (its fake network call takes 60ms);
        // window 2 (5ms) is enqueued right after it. A naive
        // "spawn every job independently" implementation would let
        // window 2's faster call land first — the single-consumer queue
        // must not.
        worker.enqueue(None, window(&[(Speaker::Other, "window-1 SLOW")]));
        worker.enqueue(None, window(&[(Speaker::Other, "window-2 fast")]));
        notified.await;

        let order = completion_order.lock().unwrap().clone();
        assert_eq!(
            order,
            vec![
                "Other: window-1 SLOW".to_string(),
                "Other: window-2 fast".to_string()
            ],
            "job 1 must finish before job 2 even though job 2's own call is faster"
        );

        let memory = store.lock().unwrap().clone();
        assert_eq!(memory.summary.len(), 2);
        assert!(
            memory.summary[0].contains("window-1 SLOW")
                && memory.summary[1].contains("window-2 fast"),
            "final summary list must stay in chronological (enqueue) order: {:?}",
            memory.summary
        );
    }

    #[test]
    fn enqueue_before_worker_init_does_not_panic() {
        // No `AppHandle`/managed state at all here — `enqueue_window` only
        // has a real app to call `try_state` against in production; this
        // test instead exercises the one piece that doesn't need Tauri at
        // all, confirming `MemoryWorker::enqueue` itself never panics after
        // its consumer task has already been dropped (a session ending
        // mid-flight).
        let processed = Arc::new(AtomicBool::new(false));
        struct MarkingRunner(Arc<AtomicBool>);
        #[async_trait::async_trait]
        impl JobRunner for MarkingRunner {
            async fn run(&self, _session_id: Option<uuid::Uuid>, _window: ConversationWindow) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let worker = MemoryWorker::spawn(Arc::new(MarkingRunner(processed)));
        worker.enqueue(None, window(&[(Speaker::Other, "hi")]));
        drop(worker);
        // No assertion beyond "this did not panic" — the consumer task may
        // or may not have gotten to the job before the sender dropped;
        // both are acceptable, a crash is not.
    }
}
