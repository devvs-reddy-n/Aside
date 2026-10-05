//! The workspace window's red and yellow buttons (2026-10-04).
//!
//! Aside never goes full screen, so the green button is hidden.
//!
//! Red quits Aside at once, with no question (see `quit_now`).
//!
//! Yellow depends on screen-share hiding:
//! - **Hiding on:** Aside has no Dock icon, and a minimized window would
//!   still appear in the Dock where a shared screen can see it. So yellow
//!   hides every Aside window instead, and Command+H brings them back. That
//!   key is only claimed while Aside is hidden this way, then released.
//! - **Hiding off:** yellow minimizes to the Dock like any other app.
//!
//! macOS has no "minimize is about to happen" hook to cancel, so the yellow
//! button's click is pointed at a tiny Objective-C object of our own instead
//! of the window's built-in minimize.

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Brings Aside back after the yellow button hid it.
pub const RESTORE_ACCELERATOR: &str = "CmdOrCtrl+H";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimizeChoice {
    /// Hide every window and wait for Command+H.
    HideApp,
    /// The ordinary Dock minimize.
    Minimize,
}

/// Pure: what the yellow button does for this screen-share setting.
pub fn minimize_choice(screen_share_hidden: bool) -> MinimizeChoice {
    if screen_share_hidden {
        MinimizeChoice::HideApp
    } else {
        MinimizeChoice::Minimize
    }
}

/// The red button: quit now. Marks the quit as already decided so the
/// exit path does not stop to ask, and capture is still torn down there.
pub fn quit_now(app: &AppHandle) {
    app.state::<crate::lifecycle::QuitDecision>().allow();
    app.exit(0);
}

/// The yellow button, and the Window menu's Minimize.
pub fn minimize(app: &AppHandle, window: &WebviewWindow) {
    match minimize_choice(crate::stealth::is_enabled_for_app(app)) {
        MinimizeChoice::Minimize => {
            let _ = window.minimize();
        }
        MinimizeChoice::HideApp => hide_app(app),
    }
}

fn hide_app(app: &AppHandle) {
    if crate::stealth::hide_on_screen(app) {
        set_restore_key(app, true);
    }
}

/// True when this key press is the Command+H that brings Aside back.
pub fn is_restore(pressed: &Shortcut) -> bool {
    RESTORE_ACCELERATOR
        .parse::<Shortcut>()
        .map(|restore| restore.id() == pressed.id())
        .unwrap_or(false)
}

/// Command+H: show the windows the yellow button hid, then give the key
/// back to other apps.
pub fn restore(app: &AppHandle) {
    crate::stealth::show_on_screen(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
    set_restore_key(app, false);
}

/// Claims or releases Command+H. Done off the main thread: the shortcut
/// plugin hands the work to the main thread and waits for it, and both
/// callers (a button click, a key press) are already on that thread.
fn set_restore_key(app: &AppHandle, claim: bool) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(shortcut) = RESTORE_ACCELERATOR.parse::<Shortcut>() else {
            return;
        };
        let shortcuts = app.global_shortcut();
        let result = if claim {
            if shortcuts.is_registered(shortcut) {
                Ok(())
            } else {
                shortcuts.register(shortcut)
            }
        } else if shortcuts.is_registered(shortcut) {
            shortcuts.unregister(shortcut)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            eprintln!("Command+H could not be {}: {error}", if claim { "claimed" } else { "released" });
        }
    });
}

/// Hide the green button and point the yellow one at [`minimize`].
#[cfg(target_os = "macos")]
pub fn install(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Err(message) = crate::panic_guard::catch("setting up the window buttons", || {
            mac::install(&app, &target);
        }) {
            eprintln!("{message}");
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn install(_window: &WebviewWindow) {}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::OnceLock;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, sel, ClassType};
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tauri::{AppHandle, Manager, WebviewWindow};

    static APP: OnceLock<AppHandle> = OnceLock::new();

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and this class
        // has no Drop.
        #[unsafe(super(NSObject))]
        #[name = "AsideMinimizeButtonTarget"]
        struct MinimizeTarget;

        impl MinimizeTarget {
            #[unsafe(method(asideMinimize:))]
            fn aside_minimize(&self, _sender: Option<&AnyObject>) {
                // Runs on AppKit's own event path; see `panic_guard`.
                if let Err(message) = crate::panic_guard::catch("the minimize button", || {
                    if let Some(app) = APP.get() {
                        if let Some(main) = app.get_webview_window("main") {
                            super::minimize(app, &main);
                        }
                    }
                }) {
                    eprintln!("{message}");
                }
            }
        }

        unsafe impl NSObjectProtocol for MinimizeTarget {}
    );

    pub fn install(app: &AppHandle, window: &WebviewWindow) {
        let _ = APP.set(app.clone());
        let Ok(pointer) = window.ns_window() else {
            return;
        };
        // SAFETY: Tauri hands back this window's live NSWindow, and we are
        // on the main thread (`run_on_main_thread`).
        let ns_window: &NSWindow = unsafe { &*(pointer as *const NSWindow) };
        if let Some(zoom) = ns_window.standardWindowButton(NSWindowButton::ZoomButton) {
            zoom.setHidden(true);
        }
        if let Some(yellow) = ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton) {
            // A button only holds its target weakly, so keep one target
            // alive for the life of the app.
            let target: Retained<MinimizeTarget> = unsafe { msg_send![MinimizeTarget::class(), new] };
            let target: &'static MinimizeTarget = Box::leak(Box::new(target));
            // SAFETY: the target lives forever and answers this selector.
            unsafe {
                yellow.setTarget(Some(target.as_ref()));
                yellow.setAction(Some(sel!(asideMinimize:)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_screen_share_hiding_on_yellow_hides_the_app() {
        assert_eq!(minimize_choice(true), MinimizeChoice::HideApp);
    }

    #[test]
    fn with_screen_share_hiding_off_yellow_is_a_normal_minimize() {
        assert_eq!(minimize_choice(false), MinimizeChoice::Minimize);
    }

    #[test]
    fn command_h_is_recognised_as_the_way_back() {
        assert!(is_restore(&"CmdOrCtrl+H".parse().unwrap()));
        assert!(!is_restore(&"CmdOrCtrl+Enter".parse().unwrap()));
        assert!(!is_restore(&"CmdOrCtrl+Shift+H".parse().unwrap()));
    }
}
