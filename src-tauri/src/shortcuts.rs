//! Global keyboard shortcuts (2026-08-25).
//!
//! **Global, not window-local, and that distinction is the whole point.**
//! During a real interview the keyboard focus is on Zoom, not on Aside's
//! little helper window. A `keydown` listener inside the panel's webview —
//! which is all this app had before — can only fire when the panel itself is
//! focused, i.e. essentially never at the moment it's actually needed.
//! `references/Repo1-Natively` registers every one of its shortcuts globally
//! and its own source cites this exact problem as a fixed regression.
//!
//! Registered from Rust rather than from JavaScript so the bindings live
//! independently of which webviews happen to be open, and so the health
//! check below sits next to the registration it repairs.
//!
//! **Two operational realities this handles, both learned from the
//! reference rather than discovered the hard way:**
//!
//! 1. Registration *silently fails* when another app already owns the
//!    accelerator — `register` returns without an error the user would ever
//!    see. So every attempt's outcome is recorded and surfaced in Settings.
//!    A shortcut that quietly does nothing is exactly the class of failure
//!    this session was opened to fix.
//! 2. macOS silently drops global bindings after sleep/wake and some
//!    workspace switches. A periodic check re-registers anything that went
//!    missing.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Answer whatever the other person last asked.
pub const ANSWER_ACCELERATOR: &str = "CmdOrCtrl+Enter";
/// Take a picture of the screen and ask the AI about it.
pub const CAPTURE_ACCELERATOR: &str = "CmdOrCtrl+Shift+Enter";
/// Steps through the live window's answer history.
pub const HISTORY_PREV_ACCELERATOR: &str = "CmdOrCtrl+Shift+Comma";
pub const HISTORY_NEXT_ACCELERATOR: &str = "CmdOrCtrl+Shift+Period";

pub const EVENT_SHORTCUT_STATUS: &str = "shortcut-status";
/// A one-line, plain-English aside shown in the panel — used when a
/// shortcut fires but there is nothing for it to act on yet.
pub const EVENT_SHORTCUT_NOTICE: &str = "shortcut-notice";
/// History events are global so navigation works while a call app has focus.
pub const EVENT_OVERLAY_HISTORY_PREV: &str = "overlay-history-prev";
pub const EVENT_OVERLAY_HISTORY_NEXT: &str = "overlay-history-next";

const HEALTH_CHECK_SECONDS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShortcutRegistration {
    pub accelerator: String,
    /// What this shortcut does, in plain language, for the Settings list.
    pub description: String,
    pub registered: bool,
    pub problem: Option<String>,
}

#[derive(Default)]
pub struct ShortcutStatus(Mutex<Vec<ShortcutRegistration>>);

fn definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        (ANSWER_ACCELERATOR, "Answer"),
        (CAPTURE_ACCELERATOR, "Screenshot answer"),
        (HISTORY_PREV_ACCELERATOR, "Previous answer"),
        (HISTORY_NEXT_ACCELERATOR, "Next answer"),
    ]
}

/// Pure: which of `now` went from working to not-working and therefore need
/// re-registering. Split out so the repair rule is testable without a real
/// app, a real keyboard, or a ten-second wait.
pub fn needs_reregistration(now: &[bool]) -> Vec<usize> {
    now.iter()
        .enumerate()
        .filter(|(_, ok)| !**ok)
        .map(|(i, _)| i)
        .collect()
}

/// Plain-language rendering for the Settings list. A conflict is by far the
/// most likely failure and gets a message that says what to do about it.
///
/// `#[allow(dead_code)]`: the Settings window renders this same wording in
/// TSX rather than calling across the bridge for one string, so production
/// code doesn't call this — but it is the single definition the tests pin
/// the wording against, which is what stops the two drifting silently.
#[allow(dead_code)]
pub fn describe(reg: &ShortcutRegistration) -> String {
    if reg.registered {
        format!("{} — {}", reg.accelerator, reg.description)
    } else {
        format!(
            "{} — not working. Another app is probably using this key combination.",
            reg.accelerator
        )
    }
}

fn set_status(app: &AppHandle, rows: Vec<ShortcutRegistration>) {
    let state = app.state::<ShortcutStatus>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    *guard = rows.clone();
    drop(guard);
    let _ = app.emit(EVENT_SHORTCUT_STATUS, rows);
}

/// Registers every shortcut, recording each outcome rather than assuming
/// success.
pub fn register_all(app: &AppHandle) {
    let mut rows = Vec::new();
    for (accel, description) in definitions() {
        let parsed: Result<Shortcut, _> = accel.parse();
        let (registered, problem) = match parsed {
            Ok(shortcut) => match app.global_shortcut().register(shortcut) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            },
            Err(e) => (false, Some(format!("unrecognized key combination: {e}"))),
        };
        if let Some(p) = &problem {
            eprintln!("global shortcut {accel} could not be registered: {p}");
        }
        rows.push(ShortcutRegistration {
            accelerator: accel.to_string(),
            description: description.to_string(),
            registered,
            problem,
        });
    }
    set_status(app, rows);
}

/// Re-registers any binding macOS has quietly dropped. Runs forever, every
/// [`HEALTH_CHECK_SECONDS`]; cheap (four `is_registered` lookups) and the only
/// thing that keeps the shortcuts alive across sleep/wake.
pub fn spawn_health_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(HEALTH_CHECK_SECONDS));
        loop {
            ticker.tick().await;
            let defs = definitions();
            let live: Vec<bool> = defs
                .iter()
                .map(|(accel, _)| {
                    accel
                        .parse::<Shortcut>()
                        .map(|s| app.global_shortcut().is_registered(s))
                        .unwrap_or(false)
                })
                .collect();
            if needs_reregistration(&live).is_empty() {
                continue;
            }
            eprintln!("re-registering global shortcuts that the system dropped");
            register_all(&app);
        }
    });
}

/// Which of our accelerators `pressed` is, if any.
///
/// **Compares parsed `Shortcut` values, never strings.** An adversarial
/// review caught the string version as a silent, total failure: the
/// underlying crate's `into_string()` re-serializes with its own lowercase
/// modifier vocabulary and fixed ordering (`"super+Enter"`,
/// `"shift+super+Enter"` on macOS), which never equals the `"CmdOrCtrl+…"`
/// spelling used to register. Registration still *succeeded*, so Settings
/// reported both shortcuts as working while every keypress fell through to
/// nothing — precisely the silently-broken state this whole session set out
/// to eliminate. Parsing both sides normalizes the spelling away.
fn match_accelerator(pressed: &Shortcut) -> Option<&'static str> {
    for (accel, _) in definitions() {
        if accel
            .parse::<Shortcut>()
            .map(|s| &s == pressed)
            .unwrap_or(false)
        {
            return Some(accel);
        }
    }
    None
}

/// Layers 6–8: Command+Enter freezes the live conversation window and
/// sends it in one request. Screen look-up (Command+Shift+Enter) stays
/// on its own gate — Layer 11.
pub fn diagnostic_cmd_enter_may_call_llm() -> bool {
    true
}

/// Screenshot answer (Command+Shift+Enter) can use the configured vision model.
pub fn diagnostic_screen_hotkey_may_call_llm() -> bool {
    true
}

/// What each accelerator actually does. Each path has its own gate so
/// turning on Command+Enter does not also turn on screen look-up.
pub fn handle(app: &AppHandle, pressed: &Shortcut) {
    match match_accelerator(pressed) {
        Some(ANSWER_ACCELERATOR) => answer_current_question(app),
        Some(CAPTURE_ACCELERATOR) => capture_and_analyze(app),
        Some(HISTORY_PREV_ACCELERATOR) => {
            let _ = app.emit(EVENT_OVERLAY_HISTORY_PREV, ());
        }
        Some(HISTORY_NEXT_ACCELERATOR) => {
            let _ = app.emit(EVENT_OVERLAY_HISTORY_NEXT, ());
        }
        _ => {}
    }
}

fn active_session_id(app: &AppHandle) -> Option<uuid::Uuid> {
    crate::active_session::get(app).and_then(|s| s.id)
}

fn answer_current_question(app: &AppHandle) {
    if !diagnostic_cmd_enter_may_call_llm() {
        return;
    }
    // Command+Enter claims, then freezes everything spoken since the last
    // press, then sends that whole window. Auto-answer stays off. No second
    // model. Freeze lives inside `respond_to_window` so a local+global
    // double-fire cannot snapshot the window and then drop it.
    let app = app.clone();
    let session_id = active_session_id(&app);
    tauri::async_runtime::spawn(async move {
        if let Err(message) = crate::llm::respond_to_window(app.clone(), session_id, None).await {
            let _ = app.emit(EVENT_SHORTCUT_NOTICE, message);
        }
    });
}

fn capture_and_analyze(app: &AppHandle) {
    if !diagnostic_screen_hotkey_may_call_llm() {
        return;
    }
    let Some(claim) = crate::llm::try_begin_user_answer(app) else {
        return;
    };
    let app = app.clone();
    let session_id = active_session_id(&app);
    tauri::async_runtime::spawn(async move {
        let encoded = crate::screen::cache::jpeg_for_answer(&app).await;

        match encoded {
            Ok(jpeg_base64) => {
                crate::llm::respond_to_screenshot(app, session_id, jpeg_base64, claim).await;
            }
            Err(message) => {
                // Includes the denied-permission case, which arrives here as
                // `screen::BLANK_FRAME_MESSAGE` — a plain-English pointer to
                // System Settings, not a black-rectangle answer.
                drop(claim);
                let _ = app.emit(EVENT_SHORTCUT_NOTICE, message);
            }
        }
    });
}

/// The plugin, ready to hand to `tauri::Builder::plugin`.
///
/// Concrete `Wry` rather than generic over the runtime, matching the rest of
/// this codebase — every other function here takes the concrete `AppHandle`
/// (which *is* `AppHandle<Wry>`), and `tauri::Builder::default()` is a
/// `Builder<Wry>`, so a generic parameter here would be a type variable with
/// exactly one possible value.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Fire on press only. Without this the handler runs twice per
            // keystroke (down and up), and the second one would be swallowed
            // by `AnswerTrigger`'s cooldown — working by accident rather
            // than on purpose.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            // This closure is documented by `tauri-plugin-global-shortcut`
            // itself ("we ensure it is run on main thread only") to run on
            // the exact thread that also pumps native macOS events. A panic
            // here, uncaught, unwinds straight into that native dispatch
            // path and force-aborts the whole app with no on-screen message
            // at all — see `panic_guard`'s doc comment for the real crash
            // this is defending against. Catching it here converts that
            // into a readable notice instead.
            if let Err(message) = crate::panic_guard::catch("a keyboard shortcut", || {
                if crate::window_buttons::is_restore(shortcut) {
                    crate::window_buttons::restore(app);
                } else {
                    handle(app, shortcut);
                }
            })
            {
                eprintln!("{message}");
                let _ = app.emit(EVENT_SHORTCUT_NOTICE, message);
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(accel: &str, registered: bool) -> ShortcutRegistration {
        ShortcutRegistration {
            accelerator: accel.to_string(),
            description: "does a thing".to_string(),
            registered,
            problem: if registered {
                None
            } else {
                Some("conflict".to_string())
            },
        }
    }

    #[test]
    fn cmd_enter_and_the_screen_hotkey_can_both_call_the_model() {
        assert!(diagnostic_cmd_enter_may_call_llm());
        assert!(diagnostic_screen_hotkey_may_call_llm());
    }

    #[test]
    fn nothing_needs_reregistering_when_everything_is_alive() {
        assert!(needs_reregistration(&[true, true]).is_empty());
    }

    #[test]
    fn a_dropped_binding_is_identified_by_index() {
        assert_eq!(needs_reregistration(&[true, false]), vec![1]);
        assert_eq!(needs_reregistration(&[false, true]), vec![0]);
    }

    #[test]
    fn all_dropped_bindings_are_identified_the_case_after_sleep_wake() {
        assert_eq!(needs_reregistration(&[false, false]), vec![0, 1]);
    }

    #[test]
    fn an_empty_set_never_asks_for_repair() {
        assert!(needs_reregistration(&[]).is_empty());
    }

    #[test]
    fn a_working_shortcut_is_described_by_what_it_does() {
        let text = describe(&reg(ANSWER_ACCELERATOR, true));
        assert!(text.contains(ANSWER_ACCELERATOR));
        assert!(text.contains("does a thing"));
    }

    #[test]
    fn only_the_four_requested_bindings_are_registered() {
        let keys: Vec<_> = definitions().into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, vec!["CmdOrCtrl+Enter", "CmdOrCtrl+Shift+Enter",
            "CmdOrCtrl+Shift+Comma", "CmdOrCtrl+Shift+Period"]);
    }

    #[test]
    fn removed_shortcuts_cannot_dispatch_an_action() {
        for old in ["CmdOrCtrl+K", "CmdOrCtrl+Shift+A", "CmdOrCtrl+H",
            "CmdOrCtrl+Shift+H", "CmdOrCtrl+[", "CmdOrCtrl+]",
            "CmdOrCtrl+P", "CmdOrCtrl+Q", "CmdOrCtrl+ArrowLeft", "CmdOrCtrl+ArrowRight"] {
            assert_eq!(match_accelerator(&old.parse().unwrap()), None, "{old}");
        }
    }

    #[test]
    fn a_broken_shortcut_says_why_in_plain_language_not_silence() {
        // The whole reason registration outcomes are tracked: a shortcut
        // that quietly does nothing is indistinguishable from a broken app.
        let text = describe(&reg(ANSWER_ACCELERATOR, false));
        assert!(text.contains("not working"));
        assert!(
            text.contains("Another app"),
            "must name the likely cause: {text}"
        );
    }

    #[test]
    fn a_pressed_shortcut_is_matched_back_to_the_accelerator_that_registered_it() {
        // REGRESSION TEST for a critical bug an adversarial review caught:
        // dispatch used to compare `shortcut.into_string()` against these
        // constants, but the crate re-serializes with its own vocabulary
        // ("super+Enter", not "CmdOrCtrl+Enter"), so NOTHING ever matched —
        // both shortcuts registered successfully, reported themselves as
        // working, and then silently did nothing forever. The old
        // "does it parse" test passed the whole time, because parsing was
        // never the broken half. This tests the actual round trip.
        for accel in [ANSWER_ACCELERATOR, CAPTURE_ACCELERATOR] {
            let pressed: Shortcut = accel.parse().unwrap();
            assert_eq!(
                match_accelerator(&pressed),
                Some(accel),
                "{accel} did not match itself after a parse round trip"
            );
        }
    }

    #[test]
    fn the_two_accelerators_never_match_each_other() {
        // ⌘↩ and ⌘⇧↩ differ only by Shift; if matching ever collapsed them,
        // pressing one would silently run the other's action.
        let answer: Shortcut = ANSWER_ACCELERATOR.parse().unwrap();
        let capture: Shortcut = CAPTURE_ACCELERATOR.parse().unwrap();
        assert_ne!(answer, capture);
        assert_eq!(match_accelerator(&answer), Some(ANSWER_ACCELERATOR));
        assert_eq!(match_accelerator(&capture), Some(CAPTURE_ACCELERATOR));
    }

    #[test]
    fn an_unrelated_shortcut_matches_nothing() {
        let other: Shortcut = "CmdOrCtrl+Shift+K".parse().unwrap();
        assert_eq!(match_accelerator(&other), None);
    }

    #[test]
    fn both_accelerators_parse_as_real_key_combinations() {
        // A typo here would register nothing and be invisible until someone
        // pressed the keys during a real interview.
        for accel in [ANSWER_ACCELERATOR, CAPTURE_ACCELERATOR] {
            assert!(
                accel.parse::<Shortcut>().is_ok(),
                "{accel} is not a valid accelerator"
            );
        }
    }

    #[test]
    fn the_two_accelerators_are_distinct() {
        assert_ne!(ANSWER_ACCELERATOR, CAPTURE_ACCELERATOR);
        // And the capture one really is the shift variant the owner asked
        // for, not a differently-shaped binding.
        assert!(CAPTURE_ACCELERATOR.contains("Shift"));
        assert!(!ANSWER_ACCELERATOR.contains("Shift"));
    }

    #[test]
    fn every_overlay_only_accelerator_parses_and_round_trips() {
        for accel in [HISTORY_PREV_ACCELERATOR, HISTORY_NEXT_ACCELERATOR] {
            let pressed: Shortcut = accel.parse().expect("must be a valid accelerator");
            assert_eq!(
                match_accelerator(&pressed),
                Some(accel),
                "{accel} did not round-trip"
            );
        }
    }

    #[test]
    fn every_definition_has_a_plain_language_description() {
        for (accel, description) in definitions() {
            assert!(!description.is_empty(), "{accel} has no description");
            // Settings shows these verbatim — no internal vocabulary.
            assert!(!description.contains("invoke"), "{accel}: {description}");
            assert!(!description.contains("LLM"), "{accel}: {description}");
        }
    }
}
