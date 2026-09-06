//! Turns an unexpected Rust panic into a plain-English on-screen notice
//! instead of a native "unexpected error" OS crash dialog.
//!
//! **Why this exists (2026-08-28).** The owner reported: pasting a job
//! description on the Documents screen, then pressing Enter, made the whole
//! app disappear with an OS-level crash dialog. Real crash reports recovered
//! from `~/Library/Logs/DiagnosticReports/Retired/aside-*.ips` (two, back to
//! back, identical stack) prove this was a genuine Rust panic that could not
//! unwind and was force-aborted (`SIGABRT` via
//! `core::panicking::panic_cannot_unwind`) — **on the main thread, inside
//! native macOS event dispatch** (`tao::platform_impl::platform::app::send_event`
//! → `-[NSApplication _handleEvent:]` → `-[NSApplication run]`), not inside
//! any Tauri command. Tauri commands (confirmed by reading
//! `tauri-macros-2.6.3/src/command/wrapper.rs`'s `body_async`, which every
//! command — sync or async — goes through) always run inside
//! `tauri::async_runtime::spawn`, i.e. on a `tokio-rt-worker` thread; the
//! crash's thread list shows all ten of those alive and unfaulted, and the
//! faulting thread is `main`. So `commands::documents::ingest_text_document`
//! and its whole call chain (independently audited: no `.unwrap()`/`.expect()`/
//! unchecked indexing on pasted text anywhere in it, `session_id: Option<Uuid>`
//! handled for `None` throughout, UTF-8-safe truncation) is not, and cannot
//! structurally be, this crash's cause.
//!
//! What *does* run directly on the main thread, as part of this exact native
//! event-dispatch path, is our own code registered as event-loop callbacks:
//! `shortcuts::plugin()`'s global-shortcut handler (its own crate,
//! `tauri-plugin-global-shortcut`, documents "we ensure it is run on main
//! thread only"), the `main` window's `on_window_event` closure, and the
//! top-level `app.run(...)` closure — all in `lib.rs`. None of these
//! contained a provable panic on inspection either, which leaves a real,
//! open possibility that the actual panic originates one level deeper, in a
//! third-party crate (`tao` 0.35.3 / `global-hotkey` 0.8.0 / `objc2`) on the
//! very new macOS beta the owner is running (`macOS 27.0`, build
//! `26A5421a`) — something this app's own source can't fix directly.
//!
//! Given that, the correct, honest response is defense in depth: catch any
//! panic that occurs in *our* code before it has a chance to unwind across
//! that native boundary and force an abort, and make sure that if it ever
//! happens again, the exact panic message and location are captured
//! somewhere the owner (and a future debugging session) can actually read —
//! not just "SIGABRT, no more information" the way this one was.

use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::sync::Once;

static INSTALL_ONCE: Once = Once::new();

/// Where panic messages are appended, one line per panic, timestamped.
/// Plain `$HOME/Library/Logs/Aside/panic.log` — this app is macOS-only
/// already (see `Cargo.toml`'s `cfg(target_os = "macos")` dependency and
/// this module's own doc comment), so no cross-platform path logic is
/// needed. Never created eagerly; the first real panic creates it.
fn log_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join("Library/Logs/Aside/panic.log"))
}

/// Installs a process-wide panic hook that appends every panic's message
/// and location to [`log_path`] before doing anything else. Idempotent —
/// safe to call more than once, only the first call takes effect.
///
/// **Must run before `tauri::Builder::default()`** so it is in place for
/// every subsequent piece of code, including plugin setup.
pub fn install() {
    INSTALL_ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = describe(info.payload());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown location".to_string());
            let line = format!("[{}] panic at {location}: {message}\n", now_iso());
            eprint!("{line}");
            if let Some(path) = log_path() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    let _ = file.write_all(line.as_bytes());
                }
            }
            // Still call the default hook (prints the usual Rust backtrace
            // guidance to stderr) — this hook only adds a durable, readable
            // record on top, it never replaces the standard one.
            default_hook(info);
        }));
    });
}

fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

/// Best-effort plain-text extraction from a panic payload — panics almost
/// always carry a `&str` or `String` message, but the type is technically
/// `dyn Any`, so this falls back to something honest rather than unwrapping.
fn describe(payload: &dyn std::any::Any) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message — the panic payload was not plain text)".to_string()
    }
}

/// Runs `f`, catching any panic instead of letting it propagate — the guard
/// that actually stops a panic in *our* code from reaching the native
/// event-dispatch boundary that force-aborts the whole process (see this
/// module's doc comment for why that boundary exists). On a caught panic,
/// returns a plain-English message naming `context` instead of the value
/// `f` would have produced.
///
/// `AssertUnwindSafe`: every caller here only closes over `&AppHandle`/
/// `&Shortcut`-shaped references used for a single synchronous call, never
/// mutated through a shared `&mut` during the call — the standard case
/// `catch_unwind`'s conservative `UnwindSafe` bound exists to warn about,
/// not a real hazard here.
pub fn catch<F: FnOnce() -> R, R>(context: &str, f: F) -> Result<R, String> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = describe(payload.as_ref());
        format!("Something unexpected went wrong in {context}: {message}. The app kept running — please try again.")
    })
}
