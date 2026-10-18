//! Confirmed close and movable bubble minimization.
//!
//! Aside never goes full screen, so the green button is hidden.
//!
//! Closing asks first; minimizing keeps a visible movable bubble.
//! Command/Ctrl+H alone hides Aside and can restore it.

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Brings Aside back after the yellow button hid it.
pub const RESTORE_ACCELERATOR: &str = "CmdOrCtrl+H";

#[derive(Default)]
pub struct BubbleState(std::sync::Mutex<Vec<String>>);

/// Minimize keeps an on-screen handle and leaves call state untouched.
pub fn minimize(app: &AppHandle, window: &WebviewWindow) {
    let bubble = match app.get_webview_window("bubble") {
        Some(bubble) => bubble,
        None => {
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                "bubble",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Aside bubble")
            .inner_size(48.0, 48.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false);
            if let Ok(position) = window.outer_position() {
                builder = builder.position(
                    position.x as f64 / window.scale_factor().unwrap_or(1.0),
                    position.y as f64 / window.scale_factor().unwrap_or(1.0),
                );
            }
            match builder.build() {
                Ok(bubble) => {
                    let owner = app.clone();
                    bubble.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            crate::lifecycle::request_quit(&owner);
                        }
                    });
                    bubble
                }
                Err(error) => {
                    eprintln!("could not create Aside bubble: {error}");
                    return;
                }
            }
        }
    };
    crate::stealth::apply_to_window(&bubble, crate::stealth::is_enabled_for_app(app));
    if bubble.show().is_err() {
        return;
    }
    let labels: Vec<String> = app
        .webview_windows()
        .into_values()
        .filter(|w| {
            w.label() != "bubble"
                && (w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(false))
        })
        .map(|w| {
            let _ = w.unminimize();
            let _ = w.hide();
            w.label().to_string()
        })
        .collect();
    remember_minimized(
        &mut app
            .state::<BubbleState>()
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner()),
        labels,
    );
}

/// Repeated native resize notifications must not erase the restore target.
fn remember_minimized(current: &mut Vec<String>, next: Vec<String>) {
    if !next.is_empty() {
        *current = next;
    }
}

#[tauri::command]
pub fn restore_bubble(app: AppHandle) {
    let labels = std::mem::take(
        &mut *app
            .state::<BubbleState>()
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner()),
    );
    for label in labels {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    if let Some(bubble) = app.get_webview_window("bubble") {
        let _ = bubble.hide();
    }
}

#[tauri::command]
pub fn toggle_visibility(app: AppHandle) {
    if crate::stealth::hide_on_screen(&app) {
        set_restore_key(&app, true);
    } else {
        restore(&app);
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
        if main.is_visible().unwrap_or(false) {
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
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
            eprintln!(
                "Command+H could not be {}: {error}",
                if claim { "claimed" } else { "released" }
            );
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
            let target: Retained<MinimizeTarget> =
                unsafe { msg_send![MinimizeTarget::class(), new] };
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
    fn repeated_minimize_keeps_the_windows_to_restore() {
        let mut windows = vec!["main".to_string(), "overlay".to_string()];
        remember_minimized(&mut windows, vec![]);
        assert_eq!(windows, vec!["main", "overlay"]);
    }

    #[test]
    fn command_h_is_recognised_as_the_way_back() {
        assert!(is_restore(&"CmdOrCtrl+H".parse().unwrap()));
        assert!(!is_restore(&"CmdOrCtrl+Enter".parse().unwrap()));
        assert!(!is_restore(&"CmdOrCtrl+Shift+H".parse().unwrap()));
    }
}
