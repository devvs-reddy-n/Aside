//! Latest screenshot, kept warm so ⌘⇧↩ does not wait to grab a new frame.
//!
//! Capture still happens only while a session is live (Connect → End).
//! Answer screenshots go through [`capture_for_answer`], which leaves Aside
//! out of the picture (OS hiding when it is on; a brief hide of our own
//! windows when it is off). The warm cache is only a speed shortcut for
//! the keyboard path and is itself taken from a display capture that the
//! OS already filters when hiding is on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::encode;
use super::CapturedImage;

/// How often to refresh the cached frame while a session is running.
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// A frame older than this is treated as stale — grab a fresh one instead
/// of sending a picture of a screen the user has already left.
const MAX_AGE: Duration = Duration::from_millis(2_500);

#[derive(Debug, Clone)]
pub struct CachedFrame {
    pub jpeg_base64: String,
    pub captured_at: Instant,
}

#[derive(Default)]
pub struct ScreenCache {
    frame: Mutex<Option<CachedFrame>>,
    running: AtomicBool,
}

impl ScreenCache {
    /// Drop the cached picture without stopping the refresh loop. A new
    /// session that starts over an existing one must not send the previous
    /// session's last frame.
    pub fn forget_frame(&self) {
        *self.frame.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.forget_frame();
    }

    pub fn latest(&self, max_age: Duration) -> Option<String> {
        let guard = self.frame.lock().unwrap_or_else(|p| p.into_inner());
        let frame = guard.as_ref()?;
        if frame.captured_at.elapsed() <= max_age {
            Some(frame.jpeg_base64.clone())
        } else {
            None
        }
    }
}

/// Starts the refresh loop if it isn't already running. Cheap to call on
/// every session start. Always forgets the previous session's frame first.
pub fn start(app: &AppHandle) {
    let cache = app.state::<ScreenCache>();
    cache.forget_frame();
    if cache.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        refresh_loop(app).await;
    });
}

pub fn stop(app: &AppHandle) {
    app.state::<ScreenCache>().stop();
}

/// Keyboard-shortcut JPEG: a fresh-enough cached frame when hiding is on
/// (those frames already leave Aside out), otherwise a live grab that
/// tucks our windows away first. The Screenshot button always uses
/// [`fresh_jpeg_for_answer`].
pub async fn jpeg_for_answer(app: &AppHandle) -> Result<String, String> {
    if crate::stealth::is_enabled_for_app(app) {
        if let Some(cached) = app.state::<ScreenCache>().latest(MAX_AGE) {
            return Ok(cached);
        }
    }
    crate::stealth::capture_without_aside(app, capture_one).await
}

/// A click on the visible Screenshot button means "what is on my screen
/// now". Do not reuse a two-second-old warm frame here: someone can switch
/// from a call page to a coding question and click immediately, which used
/// to send the prior page to the model.
pub async fn fresh_jpeg_for_answer(app: &AppHandle) -> Result<String, String> {
    crate::stealth::capture_without_aside(app, capture_one).await
}

async fn refresh_loop(app: AppHandle) {
    let mut tick = tokio::time::interval(REFRESH_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if !app.state::<ScreenCache>().running.load(Ordering::SeqCst) {
            return;
        }
        let encoded = tauri::async_runtime::spawn_blocking(capture_one).await;
        match encoded {
            Ok(Ok(jpeg_base64)) => {
                let cache = app.state::<ScreenCache>();
                if !cache.running.load(Ordering::SeqCst) {
                    return;
                }
                *cache.frame.lock().unwrap_or_else(|p| p.into_inner()) = Some(CachedFrame {
                    jpeg_base64,
                    captured_at: Instant::now(),
                });
            }
            Ok(Err(_)) => {
                // Most often: Screen Recording not granted (blank frame).
                // Don't hammer the capture path; wait for the next tick.
            }
            Err(_) => return,
        }
    }
}

fn capture_one() -> Result<String, String> {
    let frame: CapturedImage = super::capture_primary_display()?;
    encode::to_jpeg_base64(&frame, encode::TECHNICAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_frame_is_returned_and_a_stale_one_is_not() {
        let cache = ScreenCache::default();
        *cache.frame.lock().unwrap() = Some(CachedFrame {
            jpeg_base64: "abc".into(),
            captured_at: Instant::now(),
        });
        assert_eq!(cache.latest(MAX_AGE).as_deref(), Some("abc"));

        *cache.frame.lock().unwrap() = Some(CachedFrame {
            jpeg_base64: "old".into(),
            captured_at: Instant::now() - Duration::from_secs(10),
        });
        assert_eq!(cache.latest(MAX_AGE), None);
    }

    #[test]
    fn stop_clears_the_cached_frame() {
        let cache = ScreenCache::default();
        cache.running.store(true, Ordering::SeqCst);
        *cache.frame.lock().unwrap() = Some(CachedFrame {
            jpeg_base64: "abc".into(),
            captured_at: Instant::now(),
        });
        cache.stop();
        assert!(!cache.running.load(Ordering::SeqCst));
        assert!(cache.latest(MAX_AGE).is_none());
    }

    #[test]
    fn forget_frame_drops_the_picture_but_leaves_the_loop_flag() {
        let cache = ScreenCache::default();
        cache.running.store(true, Ordering::SeqCst);
        *cache.frame.lock().unwrap() = Some(CachedFrame {
            jpeg_base64: "old-session".into(),
            captured_at: Instant::now(),
        });
        cache.forget_frame();
        assert!(cache.running.load(Ordering::SeqCst));
        assert!(cache.latest(MAX_AGE).is_none());
    }
}
