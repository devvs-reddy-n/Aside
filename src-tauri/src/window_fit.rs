//! Keeps Aside's windows fully on the screen.
//!
//! A window that is taller or wider than the usable part of the screen (the
//! area left after the menu bar and Dock), or that sits partly past an edge,
//! shows up "cut in the middle". These rules shrink and move a window until
//! all of it is visible. They are plain arithmetic so they can be tested
//! without a real display.

use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Returns `win` shrunk to fit inside `area` (leaving `margin` pixels on each
/// side) and shifted so no edge is off the area. The window never shrinks
/// below `min_w` x `min_h`, even if that means it cannot fully fit.
pub fn fit_rect(win: Rect, area: Rect, margin: u32, min_w: u32, min_h: u32) -> Rect {
    let max_w = area.w.saturating_sub(margin * 2).max(min_w);
    let max_h = area.h.saturating_sub(margin * 2).max(min_h);
    let w = win.w.min(max_w);
    let h = win.h.min(max_h);

    let left = area.x + margin as i32;
    let top = area.y + margin as i32;
    let right = area.x + area.w as i32 - margin as i32;
    let bottom = area.y + area.h as i32 - margin as i32;

    let x = win.x.min(right - w as i32).max(left);
    let y = win.y.min(bottom - h as i32).max(top);
    Rect { x, y, w, h }
}

/// Fits `window` to the usable area of the screen it is on. Best effort:
/// any failure (no monitor yet, a window that is mid-close) is ignored.
pub fn fit_window(window: &WebviewWindow, min_w: u32, min_h: u32) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let work = monitor.work_area();
    let area = Rect {
        x: work.position.x,
        y: work.position.y,
        w: work.size.width,
        h: work.size.height,
    };
    let scale = monitor.scale_factor();
    let margin = (8.0 * scale).round() as u32;
    let current = Rect { x: pos.x, y: pos.y, w: size.width, h: size.height };
    let fitted = fit_rect(
        current,
        area,
        margin,
        (min_w as f64 * scale).round() as u32,
        (min_h as f64 * scale).round() as u32,
    );
    if fitted == current {
        return;
    }
    if fitted.w != current.w || fitted.h != current.h {
        let _ = window.set_size(PhysicalSize::new(fitted.w, fitted.h));
    }
    let _ = window.set_position(PhysicalPosition::new(fitted.x, fitted.y));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0, y: 25, w: 1920, h: 950 };

    #[test]
    fn a_window_that_already_fits_is_left_alone() {
        let win = Rect { x: 500, y: 100, w: 400, h: 640 };
        assert_eq!(fit_rect(win, SCREEN, 8, 340, 260), win);
    }

    #[test]
    fn a_window_taller_than_the_screen_is_shrunk_to_fit() {
        let win = Rect { x: 100, y: 25, w: 1180, h: 1200 };
        let fitted = fit_rect(win, SCREEN, 8, 900, 680);
        assert_eq!(fitted.h, 934);
        assert!(fitted.y + fitted.h as i32 <= SCREEN.y + SCREEN.h as i32 - 8);
    }

    #[test]
    fn a_window_hanging_off_the_bottom_or_right_is_pulled_back() {
        let win = Rect { x: 1700, y: 700, w: 400, h: 640 };
        let fitted = fit_rect(win, SCREEN, 8, 340, 260);
        assert_eq!(fitted.x + fitted.w as i32, 1920 - 8);
        assert_eq!(fitted.y + fitted.h as i32, 25 + 950 - 8);
    }

    #[test]
    fn a_window_above_the_menu_bar_is_pushed_down() {
        let win = Rect { x: 10, y: -30, w: 400, h: 300 };
        assert_eq!(fit_rect(win, SCREEN, 8, 340, 260).y, 33);
    }

    #[test]
    fn the_minimum_size_wins_when_the_screen_is_tiny() {
        let tiny = Rect { x: 0, y: 0, w: 500, h: 400 };
        let fitted = fit_rect(Rect { x: 0, y: 0, w: 1180, h: 860 }, tiny, 8, 900, 680);
        assert_eq!((fitted.w, fitted.h), (900, 680));
    }
}
