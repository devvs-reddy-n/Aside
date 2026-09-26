//! The Live overlay window (`future refs/live-helper-ui-spec.md` §6) —
//! created here at runtime, not declared statically in `tauri.conf.json`,
//! because its lifecycle is tied to session state: it is opened when
//! Connect is pressed and closed when the session ends, not present for
//! the app's whole lifetime the way `main` is.
//!
//! Screen-share hiding (`crate::stealth`) is applied at creation so the
//! first painted frame is already left out of captures. The overlay also
//! skips the taskbar (Windows/Linux) because it is a floating tool, not a
//! second document window.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use crate::audio::channel::Channel;
use crate::storage::{settings, Db};

pub const OVERLAY_LABEL: &str = "overlay";

/// The two shapes the live window takes (redesign, 2026-09-25).
///
/// - **Dock**: a tall companion column. Free to resize by hand; it never
///   snaps back on its own.
/// - **Capsule**: a slim bar that grows only as tall as the answer card
///   under it (the page asks for that height with `resize_overlay`).
///
/// The old window could not go narrower than 720px, but its toolbar needed
/// 772px, so End Session sat off-screen. The floors below are chosen so that
/// everything the page keeps visible at that size (Answer, Screenshot, End)
/// always fits; the page shrinks its own extras to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayMode {
    Dock,
    Capsule,
}

const DOCK_MIN_WIDTH: f64 = 340.0;
const DOCK_MIN_HEIGHT: f64 = 260.0;
const CAPSULE_MIN_WIDTH: f64 = 340.0;
const CAPSULE_MIN_HEIGHT: f64 = 64.0;
/// Starting size: the docked column, sized so the whole footer (Answer,
/// follow-up box, End Session) is visible without scrolling.
const DEFAULT_WIDTH: f64 = 400.0;
const DEFAULT_HEIGHT: f64 = 640.0;
const CAPSULE_DEFAULT_WIDTH: f64 = 460.0;
/// Gap left between the docked window and the screen's right/top edges.
const SCREEN_MARGIN: f64 = 24.0;
const SCREEN_TOP_MARGIN: f64 = 56.0;

/// The smallest window the given mode allows.
fn mode_min(mode: OverlayMode) -> (f64, f64) {
    match mode {
        OverlayMode::Dock => (DOCK_MIN_WIDTH, DOCK_MIN_HEIGHT),
        OverlayMode::Capsule => (CAPSULE_MIN_WIDTH, CAPSULE_MIN_HEIGHT),
    }
}

/// Fits a requested size to the mode: never below its floor, and a missing
/// side falls back to that mode's default.
fn mode_size(mode: OverlayMode, width: Option<f64>, height: Option<f64>) -> (f64, f64) {
    let (min_w, min_h) = mode_min(mode);
    let (default_w, default_h) = match mode {
        OverlayMode::Dock => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        OverlayMode::Capsule => (CAPSULE_DEFAULT_WIDTH, CAPSULE_MIN_HEIGHT),
    };
    (
        width.unwrap_or(default_w).max(min_w),
        height.unwrap_or(default_h).max(min_h),
    )
}

/// Opens the overlay, creating it the first time and simply re-showing +
/// focusing it on every later call (e.g. re-connecting after Edit) rather
/// than building a second window on top of one that is merely hidden.
#[tauri::command]
pub fn open_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        crate::stealth::apply_to_window(&existing, crate::stealth::is_enabled_for_app(&app));
        existing.show().map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let stealth_on = crate::stealth::is_enabled_for_app(&app);
    let window =
        WebviewWindowBuilder::new(&app, OVERLAY_LABEL, WebviewUrl::App("index.html".into()))
            .title("Aside")
            // Transparent + no decorations: the dark-glass chrome is drawn by
            // the frontend (`--ov-*` tokens, `backdrop-filter`), not by the OS
            // window frame.
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .resizable(true)
            .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .min_inner_size(DOCK_MIN_WIDTH, DOCK_MIN_HEIGHT)
            // Hidden until sharing-type is set, so the first frame is never
            // captured. Shown just below, after `apply_to_window`.
            .visible(false)
            .skip_taskbar(true)
            .content_protected(stealth_on)
            // A native shadow follows a transparent webview's changing alpha
            // outline, which is what drew the jagged black sketch around the
            // toolbar and answer card. CSS supplies clean shadows per surface.
            .shadow(false)
            .build()
            .map_err(|e| e.to_string())?;

    crate::stealth::apply_to_window(&window, stealth_on);

    // Start docked against the right edge of the screen the window opened on,
    // like a side panel. Not load-bearing: the header is a drag region, so it
    // can be moved anywhere.
    place_docked(&window);
    window.show().map_err(|e| e.to_string())?;
    crate::stealth::apply_to_window(&window, stealth_on);

    // Belt-and-suspenders, same reasoning as `lib.rs`'s shutdown teardown:
    // the in-UI End Session button (Timer/End pill) is the normal path, but
    // if the window is ever closed some other way (Mission Control, a
    // future OS-level close affordance), this stops capture instead of
    // leaving mic/system audio running with no visible UI left to stop it.
    // Does not call `prevent_close()` — the window is allowed to actually
    // close either way.
    let app_for_close = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            if let Err(message) = crate::panic_guard::catch("closing the overlay window", || {
                stop_all_capture_best_effort(&app_for_close);
                if let Some(main) = app_for_close.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }) {
                eprintln!("{message}");
            }
        }
    });

    Ok(())
}

/// Puts the window at the top-right of the usable part of its screen (below
/// the menu bar, clear of the Dock), then makes sure all of it is visible.
/// Falls back to centering if the monitor cannot be read.
fn place_docked(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        let _ = window.center();
        return;
    };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let x = work.position.x as f64 + work.size.width as f64 - (DEFAULT_WIDTH + SCREEN_MARGIN) * scale;
    let y = work.position.y as f64 + SCREEN_TOP_MARGIN * scale;
    let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    keep_on_screen(window, DOCK_MIN_WIDTH, DOCK_MIN_HEIGHT);
}

/// Shrinks and moves the window until every edge is on the screen.
fn keep_on_screen(window: &tauri::WebviewWindow, min_w: f64, min_h: f64) {
    crate::window_fit::fit_window(window, min_w as u32, min_h as u32);
}

/// Stops both capture channels and their transcription, ignoring errors
/// from a channel that was never running — used by the overlay's
/// close-window fallback above and by [`end_live_session`] below, so both
/// paths tear capture down identically instead of the fallback being a
/// hand-rolled subset.
fn stop_all_capture_best_effort(app: &AppHandle) {
    let audio_state = app.state::<crate::commands::AudioState>();
    let routing = app.state::<crate::pipeline::TranscriptionRouting>();
    let _ = crate::commands::audio::stop_mic_capture(app.clone(), audio_state, routing);
    let routing = app.state::<crate::pipeline::TranscriptionRouting>();
    let _ = crate::commands::stt::stop_channel_transcription(routing, Channel::Mic);

    let sys_state = app.state::<crate::commands::SystemAudioState>();
    let routing = app.state::<crate::pipeline::TranscriptionRouting>();
    let _ = crate::commands::audio::stop_system_audio_capture(app.clone(), sys_state, routing);
    let routing = app.state::<crate::pipeline::TranscriptionRouting>();
    let _ = crate::commands::stt::stop_channel_transcription(routing, Channel::System);
}

/// The overlay's Timer/End pill (spec §6.1 row 9, after the confirm
/// dialog): stop both capture channels, end (or clear, if this was an
/// ephemeral no-database-row session) the session, close the overlay, and
/// bring `main` back. One command instead of five separate `invoke` calls
/// from the frontend so this teardown is atomic from the caller's point of
/// view and identical to the close-window fallback above.
#[tauri::command]
pub fn end_live_session(
    app: AppHandle,
    db: State<Db>,
    session_id: Option<Uuid>,
) -> Result<(), String> {
    stop_all_capture_best_effort(&app);

    if let Some(id) = session_id {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        crate::storage::sessions::end(
            &conn,
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        )?;
        drop(conn);
        crate::rag::indexer::flush_session(&app, id);
        crate::active_session::clear_if_matches(&app, id);
        crate::window_memory::invalidate_defaults(&app);
    } else {
        crate::active_session::clear_unconditionally(&app);
        crate::window_memory::invalidate_defaults(&app);
    }

    close_overlay(app.clone())?;
    show_main_window(app)?;
    Ok(())
}

/// Closes the overlay outright (used by End Session — a fresh overlay is
/// built next time Connect is pressed, so there is nothing worth keeping
/// hidden-but-alive the way `main`'s hide-on-close does). Not a Tauri
/// command — nothing in the frontend calls it directly, only `end_session`
/// above does.
fn close_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        existing.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sets the window's shape. Called when the page switches between the docked
/// column and the capsule, with the size to restore (the page remembers the
/// docked size it last had). Both sides are optional; a missing one uses that
/// mode's default. Also moves the OS-level minimum so the window can never be
/// dragged smaller than the layout can hold.
#[tauri::command]
pub fn set_overlay_mode(
    app: AppHandle,
    mode: OverlayMode,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return Ok(());
    };
    let (min_w, min_h) = mode_min(mode);
    let (w, h) = mode_size(mode, width, height);
    window
        .set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize {
            width: min_w,
            height: min_h,
        })))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize { width: w, height: h }))
        .map_err(|e| e.to_string())?;
    keep_on_screen(&window, min_w, min_h);
    Ok(())
}

/// The window's current inner size in logical pixels, so the page can
/// remember the docked size before it collapses to a capsule.
#[tauri::command]
pub fn get_overlay_size(app: AppHandle) -> Result<(f64, f64), String> {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return Ok((DEFAULT_WIDTH, DEFAULT_HEIGHT));
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let logical = size.to_logical::<f64>(scale);
    Ok((logical.width, logical.height))
}

/// Asks for an exact window size. In the capsule the page uses this to make
/// the window exactly as tall as its content. It is never called in the docked
/// layout, so a size the person dragged to is never overridden.
#[tauri::command]
pub fn resize_overlay(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return Ok(());
    };
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: width.max(CAPSULE_MIN_WIDTH),
            height: height.max(CAPSULE_MIN_HEIGHT),
        }))
        .map_err(|e| e.to_string())?;
    keep_on_screen(&window, CAPSULE_MIN_WIDTH, CAPSULE_MIN_HEIGHT);
    Ok(())
}

/// Connect (spec §5): "hides Create (do not destroy form state)" — `main`
/// stays alive with all its React state intact, it just stops being shown
/// while the overlay is the visible window.
#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        main.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// End Session (spec §6.1 row 9) and the More menu's Dashboard/Edit rows
/// all return to `main` this way.
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        main.show().map_err(|e| e.to_string())?;
        main.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// --- Overlay prefs (spec §9 rows 3-5: Zoom / Opacity / Font size) ---------
//
// "Persist Zoom/Opacity/Font in local settings (not per session)" (spec
// §9's closing line) — plain key/value rows via `storage::settings`, the
// same mechanism every other persisted local preference in this app
// already uses (e.g. `commands::llm::get_llm_model`/`set_llm_model`).

const KEY_ZOOM: &str = "overlay_zoom_pct";
const KEY_OPACITY: &str = "overlay_opacity_pct";
const KEY_FONT: &str = "overlay_font_px";
/// The More menu's Language picker (2026-08-30, owner request — it used to
/// be local-only React state that no command ever read). Read by
/// `commands::stt::build_engine`'s Speechmatics branch; Parakeet is a fixed
/// local model with no runtime language switch, so this only actually
/// changes transcription for Speechmatics.
const KEY_LANGUAGE: &str = "overlay_language";

const DEFAULT_ZOOM: u32 = 100;
const DEFAULT_OPACITY: u32 = 82;
const DEFAULT_FONT_PX: u32 = 13;
const DEFAULT_LANGUAGE: &str = "en";

const MIN_ZOOM: u32 = 80;
const MAX_ZOOM: u32 = 160;
/// Spec §0's hard rule: "Opacity floor at 55%. Never allow a near-invisible
/// overlay." Enforced here, not just in the frontend stepper, so nothing
/// (a stray direct `invoke` call, a future bug in the stepper) can ever
/// persist a value below it.
const MIN_OPACITY: u32 = 55;
const MAX_OPACITY: u32 = 95;
const MIN_FONT_PX: u32 = 12;
const MAX_FONT_PX: u32 = 20;

/// ISO 639-1 codes only — matches what `stt::speechmatics::SpeechmaticsConfig`/
/// `stt::deepgram::DeepgramConfig` already expect on the wire. Any value this
/// project doesn't recognize falls back to `DEFAULT_LANGUAGE` rather than
/// being sent to the STT provider unchecked.
const SUPPORTED_LANGUAGES: [&str; 6] = ["en", "es", "fr", "de", "hi", "pt"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayPrefs {
    pub zoom_pct: u32,
    pub opacity_pct: u32,
    pub font_px: u32,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        Self {
            zoom_pct: DEFAULT_ZOOM,
            opacity_pct: DEFAULT_OPACITY,
            font_px: DEFAULT_FONT_PX,
            language: default_language(),
        }
    }
}

fn clamp_prefs(raw: OverlayPrefs) -> OverlayPrefs {
    OverlayPrefs {
        zoom_pct: raw.zoom_pct.clamp(MIN_ZOOM, MAX_ZOOM),
        opacity_pct: raw.opacity_pct.clamp(MIN_OPACITY, MAX_OPACITY),
        font_px: raw.font_px.clamp(MIN_FONT_PX, MAX_FONT_PX),
        language: if SUPPORTED_LANGUAGES.contains(&raw.language.as_str()) {
            raw.language
        } else {
            default_language()
        },
    }
}

fn parse_or(value: Option<String>, fallback: u32) -> u32 {
    value
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(fallback)
}

#[tauri::command]
pub fn get_overlay_prefs(db: State<Db>) -> Result<OverlayPrefs, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let raw = OverlayPrefs {
        zoom_pct: parse_or(settings::get(&conn, KEY_ZOOM)?, DEFAULT_ZOOM),
        opacity_pct: parse_or(settings::get(&conn, KEY_OPACITY)?, DEFAULT_OPACITY),
        font_px: parse_or(settings::get(&conn, KEY_FONT)?, DEFAULT_FONT_PX),
        language: settings::get(&conn, KEY_LANGUAGE)?.unwrap_or_else(default_language),
    };
    Ok(clamp_prefs(raw))
}

#[tauri::command]
pub fn set_overlay_prefs(db: State<Db>, prefs: OverlayPrefs) -> Result<OverlayPrefs, String> {
    let clamped = clamp_prefs(prefs);
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(&conn, KEY_ZOOM, &clamped.zoom_pct.to_string())?;
    settings::set(&conn, KEY_OPACITY, &clamped.opacity_pct.to_string())?;
    settings::set(&conn, KEY_FONT, &clamped.font_px.to_string())?;
    settings::set(&conn, KEY_LANGUAGE, &clamped.language)?;
    Ok(clamped)
}

/// The persisted overlay language, for `commands::stt::build_engine` to read
/// without needing its own copy of `KEY_LANGUAGE`/`SUPPORTED_LANGUAGES`.
pub fn current_language(conn: &Connection) -> String {
    settings::get(conn, KEY_LANGUAGE)
        .ok()
        .flatten()
        .filter(|v| SUPPORTED_LANGUAGES.contains(&v.as_str()))
        .unwrap_or_else(default_language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_docked_window_can_be_narrower_than_the_old_720_floor_that_hid_end_session() {
        let (w, _) = mode_size(OverlayMode::Dock, Some(300.0), Some(300.0));
        assert_eq!(w, DOCK_MIN_WIDTH);
        assert!(DOCK_MIN_WIDTH < 720.0);
    }

    #[test]
    fn a_missing_size_uses_the_mode_default_and_a_tiny_one_is_lifted_to_the_floor() {
        assert_eq!(
            mode_size(OverlayMode::Dock, None, None),
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
        assert_eq!(
            mode_size(OverlayMode::Capsule, Some(10.0), Some(10.0)),
            (CAPSULE_MIN_WIDTH, CAPSULE_MIN_HEIGHT)
        );
    }

    #[test]
    fn the_capsule_floor_is_a_slim_bar_but_the_dock_floor_keeps_room_for_the_footer() {
        assert!(CAPSULE_MIN_HEIGHT < DOCK_MIN_HEIGHT);
        assert_eq!(mode_min(OverlayMode::Dock).1, DOCK_MIN_HEIGHT);
    }

    #[test]
    fn opacity_can_never_be_saved_below_the_spec_floor() {
        let saved = clamp_prefs(OverlayPrefs {
            zoom_pct: 100,
            opacity_pct: 1,
            font_px: 13,
            language: default_language(),
        });
        assert_eq!(
            saved.opacity_pct, MIN_OPACITY,
            "opacity must clamp up to the 55% floor"
        );
    }

    #[test]
    fn every_field_clamps_to_its_own_spec_range() {
        let saved = clamp_prefs(OverlayPrefs {
            zoom_pct: 999,
            opacity_pct: 999,
            font_px: 999,
            language: default_language(),
        });
        assert_eq!(saved.zoom_pct, MAX_ZOOM);
        assert_eq!(saved.opacity_pct, MAX_OPACITY);
        assert_eq!(saved.font_px, MAX_FONT_PX);
    }

    #[test]
    fn defaults_match_the_spec_exactly() {
        let defaults = OverlayPrefs::default();
        assert_eq!(defaults.zoom_pct, 100);
        assert_eq!(defaults.opacity_pct, 82);
        assert_eq!(defaults.font_px, 13);
        assert_eq!(defaults.language, "en");
    }

    #[test]
    fn an_unrecognized_language_falls_back_to_the_default_instead_of_being_saved_as_is() {
        let saved = clamp_prefs(OverlayPrefs {
            zoom_pct: 100,
            opacity_pct: 82,
            font_px: 13,
            language: "klingon".to_string(),
        });
        assert_eq!(saved.language, "en");
    }

    #[test]
    fn a_supported_language_is_kept_exactly() {
        let saved = clamp_prefs(OverlayPrefs {
            zoom_pct: 100,
            opacity_pct: 82,
            font_px: 13,
            language: "hi".to_string(),
        });
        assert_eq!(saved.language, "hi");
    }
}
