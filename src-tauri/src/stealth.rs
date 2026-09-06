//! Hide Aside from screenshots, recordings, meeting screen-shares, the
//! Dock, and the menu bar.
//!
//! The window stays on this computer's display. Other apps that capture
//! the screen — Zoom, Meet, Teams, QuickTime, Cmd+Shift+3 — do not see it.
//! macOS does this with `NSWindowSharingTypeNone`; Windows does it with
//! `WDA_EXCLUDEFROMCAPTURE`. Tauri's `set_content_protected` is the same
//! call the reference app makes through Electron's `setContentProtection`.
//!
//! The Dock icon and the Aside menu-bar menus go away too (macOS accessory
//! policy; Windows skip-taskbar). Activity Monitor still lists the process
//! as Aside on purpose — you can always find it and quit it yourself. This
//! file never renames the process or otherwise hides it from Activity
//! Monitor.
//!
//! Default is on. The Settings toggle is the only on/off for this flag.
//! Changing it saves the flag and relaunches so Dock / menu-bar policy
//! apply from the first frame. ⌘⇧H is a different control: it hides or
//! shows Aside on *your* screen (`toggle_on_screen`) without changing
//! screen-share hiding.

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::storage::{settings, Db};

/// Kept as the frontend's listener name. A Settings change relaunches
/// instead of emitting this, so the next process reads the saved flag.
pub const EVENT_STEALTH_CHANGED: &str = "stealth-changed";
const KEY: &str = "screen_share_invisible";
const DEFAULT: bool = true;

/// How long to wait after hiding our windows before taking a screenshot,
/// so the compositor has dropped them from the frame. The reference app
/// measured ≤30ms on Windows and kept 150ms as margin; we use the same.
const HIDE_FOR_CAPTURE_MS: u64 = 150;

pub fn parse_enabled(value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None => DEFAULT,
        Some(v) if v.eq_ignore_ascii_case("false") || v == "0" || v.eq_ignore_ascii_case("no") => {
            false
        }
        Some(v) if v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes") => {
            true
        }
        // Unknown stored value: keep the product default (hidden).
        Some(_) => DEFAULT,
    }
}

pub fn is_enabled(conn: &Connection) -> bool {
    parse_enabled(settings::get(conn, KEY).ok().flatten().as_deref())
}

pub fn is_enabled_for_app(app: &AppHandle) -> bool {
    let db = app.state::<Db>();
    let enabled = match db.0.lock() {
        Ok(conn) => is_enabled(&conn),
        Err(_) => DEFAULT,
    };
    enabled
}

pub fn apply_to_window(window: &WebviewWindow, enabled: bool) {
    if let Err(error) = window.set_content_protected(enabled) {
        eprintln!("could not apply screen-share hiding: {error}");
    }
    // The whole point of being invisible to a meeting share is that Aside
    // can still float above the meeting itself. Without this it behaves
    // like an ordinary window and disappears the moment another app is
    // clicked into or made fullscreen — leaving ⌘⇧H (`toggle_on_screen`)
    // as the *only* thing that should ever make it go away. The overlay
    // already stays on top unconditionally for its own reason (it floats
    // over a live call regardless of this setting), so it must be left out
    // here — this only changes the main window's normal layering.
    if window.label() != crate::commands::overlay::OVERLAY_LABEL {
        if let Err(error) = window.set_always_on_top(enabled) {
            eprintln!("could not update always-on-top: {error}");
        }
        if let Err(error) = window.set_visible_on_all_workspaces(enabled) {
            eprintln!("could not update all-workspaces visibility: {error}");
        }
    }
}

pub fn apply_to_all_windows(app: &AppHandle, enabled: bool) {
    for (_, window) in app.webview_windows() {
        apply_to_window(&window, enabled);
        // Overlay is a floating tool and never belongs on the taskbar.
        // The main window follows the hiding flag so Windows has no
        // taskbar button while hiding is on.
        let skip = enabled || window.label() == "overlay";
        if let Err(error) = window.set_skip_taskbar(skip) {
            eprintln!("could not update taskbar hiding: {error}");
        }
    }
    apply_app_presence(app, enabled);
}

/// Dock + menu bar only. Must not hide the windows themselves — that is
/// ⌘⇧H (`toggle_on_screen`). We do **not** call `set_dock_visibility`:
/// that uses `TransformProcessType` to UIElement, which takes the windows
/// off the screen with the Dock icon. Accessory policy plus `LSUIElement`
/// already removes the Dock icon and the Aside menus.
fn apply_app_presence(app: &AppHandle, hidden: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if hidden {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        let previously_visible: Vec<_> = app
            .webview_windows()
            .into_iter()
            .filter(|(_, window)| window.is_visible().unwrap_or(false))
            .map(|(_, window)| window)
            .collect();
        if let Err(error) = app.set_activation_policy(policy) {
            eprintln!("could not update Dock and menu-bar hiding: {error}");
        }
        for window in previously_visible {
            let _ = window.show();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, hidden);
    }
}

fn persist(conn: &Connection, enabled: bool) -> Result<(), String> {
    settings::set(conn, KEY, if enabled { "true" } else { "false" })
}

fn persist_enabled(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let current = is_enabled(&conn);
    if current != enabled {
        persist(&conn, enabled)?;
    }
    Ok(current)
}

/// Windows we hid for a screenshot, restored when this value is dropped
/// so a failed capture cannot leave Aside invisible on the real display.
pub struct HiddenWindows {
    windows: Vec<WebviewWindow>,
}

impl HiddenWindows {
    pub fn hide(app: &AppHandle) -> Self {
        let mut windows = Vec::new();
        for (_, window) in app.webview_windows() {
            if window.is_visible().unwrap_or(false) {
                let _ = window.hide();
                windows.push(window);
            }
        }
        Self { windows }
    }
}

impl Drop for HiddenWindows {
    fn drop(&mut self) {
        for window in &self.windows {
            let _ = window.show();
        }
    }
}

/// Grab a still of the screen for an answer. When hiding is on, the OS
/// already leaves Aside out of the picture. When it is off, we hide our
/// windows for a beat first so the helper never photographs itself.
pub async fn capture_without_aside(
    app: &AppHandle,
    grab: impl FnOnce() -> Result<String, String> + Send + 'static,
) -> Result<String, String> {
    let hide = !is_enabled_for_app(app);
    let _guard = if hide {
        Some(HiddenWindows::hide(app))
    } else {
        None
    };
    if hide {
        tokio::time::sleep(std::time::Duration::from_millis(HIDE_FOR_CAPTURE_MS)).await;
    }
    tauri::async_runtime::spawn_blocking(grab)
        .await
        .map_err(|e| format!("Screen capture failed: {e}"))?
}

#[tauri::command]
pub fn get_stealth(db: State<Db>) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    Ok(is_enabled(&conn))
}

#[tauri::command]
pub fn set_stealth(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let previous = persist_enabled(&app, enabled)?;
    if previous != enabled {
        let _ = app.emit(EVENT_STEALTH_CHANGED, enabled);
        // Dock / menu-bar policy is applied at launch from the saved flag.
        // Changing it mid-process was leaving the window, Dock, or menus
        // in the wrong state, so a change always relaunches.
        relaunch(&app);
    }
    Ok(enabled)
}

/// Relaunches Aside so the just-saved flag applies from a fresh process.
///
/// Tauri's own `AppHandle::request_restart()` re-executes the binary
/// directly (`Command::new(path).spawn()`, see `tauri::process::restart`)
/// rather than asking macOS to open the app bundle. In practice that raced
/// with this process's own shutdown: the new copy would flash on screen
/// and die within about a second, leaving nothing running and forcing a
/// manual reopen from Finder/Spotlight. Asking macOS itself to launch a
/// fresh, independent instance of the `.app` bundle (`open -n`) — the same
/// thing Finder does on a double-click — sidesteps that race. `app.exit`
/// with `RESTART_EXIT_CODE` keeps the existing "don't show the Quit
/// dialog for this" and capture-teardown behavior (see `lifecycle.rs`,
/// `lib.rs`'s `RunEvent` handling) without also letting Tauri's own
/// raw-binary restart fire afterward — that only happens on the
/// `request_restart` path, which this only falls back to.
fn relaunch(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if relaunch_via_open(app) {
            app.exit(tauri::RESTART_EXIT_CODE);
            return;
        }
    }
    app.request_restart();
}

/// `open -n <bundle>`, given the running binary is inside
/// `<bundle>.app/Contents/MacOS/<binary>`. Returns false (falling back to
/// Tauri's own restart, e.g. for a `cargo run` dev binary with no `.app`
/// bundle around it) if that layout, or the `open` call itself, doesn't
/// check out.
#[cfg(target_os = "macos")]
fn relaunch_via_open(app: &AppHandle) -> bool {
    let Ok(exe) = tauri::process::current_binary(&app.env()) else {
        return false;
    };
    let Some(bundle) = bundle_path_from_binary(&exe) else {
        return false;
    };
    std::process::Command::new("open")
        .arg("-n")
        .arg(bundle)
        .spawn()
        .is_ok()
}

/// Given `.../Aside.app/Contents/MacOS/aside`, returns `.../Aside.app`.
/// `None` for anything not shaped like a binary inside an app bundle (a
/// `cargo run`/`cargo test` dev binary, for instance) — the caller falls
/// back to Tauri's own restart in that case.
#[cfg(target_os = "macos")]
fn bundle_path_from_binary(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?.to_path_buf();
    (bundle.extension().and_then(|ext| ext.to_str()) == Some("app")).then_some(bundle)
}

/// Title-bar minus — same action as ⌘⇧H.
#[tauri::command(rename = "toggle_on_screen")]
pub fn toggle_on_screen_command(app: AppHandle) {
    toggle_on_screen(&app);
}

/// Labels of windows ⌘⇧H hid, so the next press can bring back exactly
/// those and not a window the person had already closed.
#[derive(Default)]
pub struct OnScreenHidden(Mutex<Vec<String>>);

/// Pure: given the windows currently on screen and the ones we hid last
/// time, either hide the live ones or restore the hidden list.
pub fn next_on_screen_action(
    currently_visible: &[String],
    previously_hidden: &[String],
) -> OnScreenAction {
    if previously_hidden.is_empty() {
        OnScreenAction::Hide(currently_visible.to_vec())
    } else {
        OnScreenAction::Show(previously_hidden.to_vec())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnScreenAction {
    Hide(Vec<String>),
    Show(Vec<String>),
}

/// ⌘⇧H — hide Aside from *your* screen, or bring it back. Does not change
/// screen-share hiding, the Dock, or Activity Monitor. Capture keeps running.
pub fn toggle_on_screen(app: &AppHandle) {
    let state = app.state::<OnScreenHidden>();
    let mut hidden = state.0.lock().unwrap_or_else(|p| p.into_inner());
    let visible: Vec<String> = app
        .webview_windows()
        .into_iter()
        .filter(|(_, window)| window.is_visible().unwrap_or(false))
        .map(|(label, _)| label)
        .collect();
    match next_on_screen_action(&visible, &hidden) {
        OnScreenAction::Hide(labels) => {
            for label in &labels {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.hide();
                }
            }
            *hidden = labels;
        }
        OnScreenAction::Show(labels) => {
            for label in &labels {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.show();
                }
            }
            hidden.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    #[cfg(target_os = "macos")]
    #[test]
    fn bundle_path_is_three_levels_above_the_binary() {
        assert_eq!(
            bundle_path_from_binary(std::path::Path::new(
                "/Applications/Aside.app/Contents/MacOS/aside"
            )),
            Some(std::path::PathBuf::from("/Applications/Aside.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_dev_binary_outside_any_app_bundle_falls_back() {
        assert_eq!(
            bundle_path_from_binary(std::path::Path::new(
                "/Users/x/Aside/src-tauri/target/debug/aside"
            )),
            None
        );
    }

    #[test]
    fn missing_setting_defaults_to_hidden() {
        assert!(parse_enabled(None));
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(is_enabled(&conn));
    }

    #[test]
    fn false_spellings_turn_hiding_off() {
        assert!(!parse_enabled(Some("false")));
        assert!(!parse_enabled(Some("FALSE")));
        assert!(!parse_enabled(Some("0")));
        assert!(!parse_enabled(Some("no")));
        assert!(!parse_enabled(Some("  false  ")));
    }

    #[test]
    fn true_spellings_keep_hiding_on() {
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some("TRUE")));
        assert!(parse_enabled(Some("1")));
        assert!(parse_enabled(Some("yes")));
    }

    #[test]
    fn garbage_falls_back_to_hidden_rather_than_showing() {
        assert!(parse_enabled(Some("maybe")));
        assert!(parse_enabled(Some("")));
    }

    #[test]
    fn set_then_get_round_trips_through_settings() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        persist(&conn, false).unwrap();
        assert!(!is_enabled(&conn));
        persist(&conn, true).unwrap();
        assert!(is_enabled(&conn));
    }

    #[test]
    fn hide_delay_matches_the_reference_app() {
        assert_eq!(HIDE_FOR_CAPTURE_MS, 150);
    }

    #[test]
    fn overlay_is_the_window_that_always_skips_the_taskbar() {
        // Must stay in lockstep with `commands::overlay::OVERLAY_LABEL`.
        assert_eq!(crate::commands::overlay::OVERLAY_LABEL, "overlay");
    }

    #[test]
    fn first_on_screen_press_hides_whatever_is_showing() {
        assert_eq!(
            next_on_screen_action(&["main".into(), "overlay".into()], &[]),
            OnScreenAction::Hide(vec!["main".into(), "overlay".into()])
        );
    }

    #[test]
    fn second_on_screen_press_restores_exactly_what_was_hidden() {
        assert_eq!(
            next_on_screen_action(&[], &["overlay".into()]),
            OnScreenAction::Show(vec!["overlay".into()])
        );
    }

    #[test]
    fn on_screen_press_does_not_invent_windows_that_were_already_closed() {
        assert_eq!(
            next_on_screen_action(&["main".into()], &["overlay".into()]),
            OnScreenAction::Show(vec!["overlay".into()])
        );
    }

    #[test]
    fn stealth_changed_event_name_matches_the_frontend_listener() {
        assert_eq!(EVENT_STEALTH_CHANGED, "stealth-changed");
    }
}
