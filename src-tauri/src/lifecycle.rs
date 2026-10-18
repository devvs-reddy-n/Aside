//! App-wide hide, quit, and relaunch.
//!
//! The title-bar buttons live in `window_buttons`: close asks first,
//! minimize keeps a movable bubble on screen.
//! The Aside menu's Quit item asks first, then quits.
//! Changing screen-share hiding relaunches so Dock and menu-bar policy
//! apply from a fresh process.

use std::sync::Mutex;

use tauri::menu::{
    AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, Manager};

pub const QUIT_MENU_ID: &str = "aside-quit";
pub const EVENT_QUIT_REQUESTED: &str = "quit-requested";

#[derive(Default)]
pub struct QuitDecision {
    confirmed: Mutex<bool>,
}

impl QuitDecision {
    pub fn allow(&self) {
        *self.confirmed.lock().unwrap_or_else(|p| p.into_inner()) = true;
    }

    pub fn is_allowed(&self) -> bool {
        *self.confirmed.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// True when this exit should wait for the on-screen Quit dialog.
///
/// A confirmed Quit, and the stealth-toggle relaunch (`RESTART_EXIT_CODE`),
/// must not show that dialog — they already know what they are doing.
pub fn exit_needs_confirmation(code: Option<i32>, already_confirmed: bool) -> bool {
    if already_confirmed {
        return false;
    }
    code != Some(tauri::RESTART_EXIT_CODE)
}

/// Bring the workspace back (so the confirm dialog can be seen) and tell
/// the frontend to ask. Idempotent if the dialog is already open.
pub fn request_quit(app: &AppHandle) {
    crate::window_buttons::restore(app);
    crate::window_buttons::restore_bubble(app.clone());
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
    let _ = app.emit(EVENT_QUIT_REQUESTED, ());
}

#[tauri::command(rename = "request_quit")]
pub fn request_quit_command(app: AppHandle) {
    request_quit(&app);
}

#[tauri::command]
pub fn confirm_quit(app: AppHandle) {
    app.state::<QuitDecision>().allow();
    app.exit(0);
}

/// Clickable window commands; all custom key bindings live in shortcuts.rs.
pub fn handle_menu(app: &AppHandle, id: &str) {
    if id == QUIT_MENU_ID {
        request_quit(app);
        return;
    }
    if id == "aside-hide" {
        crate::window_buttons::toggle_visibility(app.clone());
        return;
    }
    let window = app
        .webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false))
        .or_else(|| app.get_webview_window("main"));
    if let Some(window) = window {
        match id {
            "aside-minimize" => crate::window_buttons::minimize(app, &window),
            "aside-close" => {
                let _ = window.close();
            }
            _ => {}
        }
    }
}

/// Menu actions have no app accelerators. Standard text editing remains native.
/// No Zoom or Full Screen: Aside's windows keep the size they were given.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let pkg_info = app.package_info();
    let config = app.config();
    let about_metadata = AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };

    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "Quit Aside", true, Some("CmdOrCtrl+Q"))?;

    let minimize = MenuItem::with_id(app, "aside-minimize", "Minimize", true, None::<&str>)?;
    let close = MenuItem::with_id(app, "aside-close", "Close Window", true, None::<&str>)?;
    #[cfg(target_os = "macos")]
    let hide = MenuItem::with_id(app, "aside-hide", "Hide Aside", true, Some("CmdOrCtrl+H"))?;

    let window_menu = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &minimize,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &close,
        ],
    )?;

    let help_menu = Submenu::with_id_and_items(
        app,
        HELP_SUBMENU_ID,
        "Help",
        true,
        &[
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
        ],
    )?;

    Menu::with_items(
        app,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                pkg_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &hide,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?,
            #[cfg(not(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            )))]
            &Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &close,
                    #[cfg(not(target_os = "macos"))]
                    &quit,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &window_menu,
            &help_menu,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_quit_asks_first() {
        assert!(exit_needs_confirmation(None, false));
        assert!(exit_needs_confirmation(Some(0), false));
    }

    #[test]
    fn confirming_quit_does_not_ask_again() {
        assert!(!exit_needs_confirmation(None, true));
        assert!(!exit_needs_confirmation(Some(0), true));
    }

    #[test]
    fn stealth_relaunch_does_not_ask() {
        assert!(!exit_needs_confirmation(
            Some(tauri::RESTART_EXIT_CODE),
            false
        ));
        assert!(!exit_needs_confirmation(
            Some(tauri::RESTART_EXIT_CODE),
            true
        ));
    }

    #[test]
    fn quit_menu_id_is_the_string_the_handler_matches() {
        assert_eq!(QUIT_MENU_ID, "aside-quit");
    }

    #[test]
    fn quit_requested_event_name_matches_the_frontend_listener() {
        assert_eq!(EVENT_QUIT_REQUESTED, "quit-requested");
    }
}
