//! macOS does not implement Tauri's native resize-drag command. Track a single
//! held mouse drag instead; all other platforms keep their native operation.

use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
pub enum Edge {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[cfg(any(target_os = "macos", test))]
fn dragged(
    start: crate::window_fit::Rect,
    edge: Edge,
    dx: i32,
    dy: i32,
    min_w: u32,
    min_h: u32,
) -> crate::window_fit::Rect {
    use Edge::*;
    let west = matches!(edge, West | NorthWest | SouthWest);
    let east = matches!(edge, East | NorthEast | SouthEast);
    let north = matches!(edge, North | NorthWest | NorthEast);
    let south = matches!(edge, South | SouthWest | SouthEast);
    let w = (start.w as i64
        + if west {
            -(dx as i64)
        } else if east {
            dx as i64
        } else {
            0
        })
    .max(min_w as i64) as u32;
    let h = (start.h as i64
        + if north {
            -(dy as i64)
        } else if south {
            dy as i64
        } else {
            0
        })
    .max(min_h as i64) as u32;
    crate::window_fit::Rect {
        x: if west {
            start.x + start.w as i32 - w as i32
        } else {
            start.x
        },
        y: if north {
            start.y + start.h as i32 - h as i32
        } else {
            start.y
        },
        w,
        h,
    }
}

#[tauri::command]
pub async fn drag_resize_overlay(window: tauri::WebviewWindow, edge: Edge) -> Result<(), String> {
    if window.label() != super::overlay::OVERLAY_LABEL {
        return Err("Only the live window can be resized here.".into());
    }
    #[cfg(target_os = "macos")]
    return tauri::async_runtime::spawn_blocking(move || track_drag(window, edge))
        .await
        .map_err(|error| error.to_string())?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = edge;
        Err("Use native window resizing on this platform.".into())
    }
}

#[cfg(target_os = "macos")]
fn track_drag(window: tauri::WebviewWindow, edge: Edge) -> Result<(), String> {
    use crate::window_fit::Rect;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tauri::{PhysicalPosition, PhysicalSize};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    }
    static DRAGGING: AtomicBool = AtomicBool::new(false);
    if DRAGGING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            DRAGGING.store(false, Ordering::Release);
        }
    }
    let _release = Release;
    let origin = window.cursor_position().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let start = Rect {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    };
    let mut previous = start;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        // Combined-session state (0), left button (0). This reads button state
        // only for this explicit drag; it installs no global input listener.
        let held = unsafe { CGEventSourceButtonState(0, 0) };
        let cursor = window.cursor_position().map_err(|e| e.to_string())?;
        let next = dragged(
            start,
            edge,
            (cursor.x - origin.x).round() as i32,
            (cursor.y - origin.y).round() as i32,
            (super::overlay::DOCK_MIN_WIDTH * scale).round() as u32,
            (super::overlay::DOCK_MIN_HEIGHT * scale).round() as u32,
        );
        if next != previous {
            window
                .set_size(PhysicalSize::new(next.w, next.h))
                .map_err(|e| e.to_string())?;
            if next.x != previous.x || next.y != previous.y {
                window
                    .set_position(PhysicalPosition::new(next.x, next.y))
                    .map_err(|e| e.to_string())?;
            }
            previous = next;
        }
        if !held || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_fit::Rect;
    const START: Rect = Rect {
        x: 100,
        y: 200,
        w: 400,
        h: 640,
    };

    #[test]
    fn every_edge_moves_only_its_requested_sides() {
        use Edge::*;
        for (edge, expected) in [
            (
                North,
                Rect {
                    x: 100,
                    y: 230,
                    w: 400,
                    h: 610,
                },
            ),
            (
                NorthEast,
                Rect {
                    x: 100,
                    y: 230,
                    w: 420,
                    h: 610,
                },
            ),
            (
                East,
                Rect {
                    x: 100,
                    y: 200,
                    w: 420,
                    h: 640,
                },
            ),
            (
                SouthEast,
                Rect {
                    x: 100,
                    y: 200,
                    w: 420,
                    h: 670,
                },
            ),
            (
                South,
                Rect {
                    x: 100,
                    y: 200,
                    w: 400,
                    h: 670,
                },
            ),
            (
                SouthWest,
                Rect {
                    x: 120,
                    y: 200,
                    w: 380,
                    h: 670,
                },
            ),
            (
                West,
                Rect {
                    x: 120,
                    y: 200,
                    w: 380,
                    h: 640,
                },
            ),
            (
                NorthWest,
                Rect {
                    x: 120,
                    y: 230,
                    w: 380,
                    h: 610,
                },
            ),
        ] {
            assert_eq!(dragged(START, edge, 20, 30, 340, 260), expected);
        }
    }

    #[test]
    fn minimum_size_keeps_the_opposite_corner_fixed() {
        assert_eq!(
            dragged(START, Edge::NorthWest, 900, 900, 340, 260),
            Rect {
                x: 160,
                y: 580,
                w: 340,
                h: 260
            }
        );
        assert_eq!(
            dragged(START, Edge::SouthEast, -900, -900, 340, 260),
            Rect {
                x: 100,
                y: 200,
                w: 340,
                h: 260
            }
        );
    }

    #[test]
    fn large_sizes_are_free_and_negative_monitor_positions_work() {
        let start = Rect {
            x: -1000,
            y: -400,
            w: 800,
            h: 1280,
        };
        assert_eq!(
            dragged(start, Edge::West, -500, 999, 680, 520),
            Rect {
                x: -1500,
                y: -400,
                w: 1300,
                h: 1280
            }
        );
    }
}
