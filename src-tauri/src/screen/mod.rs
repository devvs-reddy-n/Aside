//! Screen capture for the "look at my screen and help" shortcut
//! (2026-08-25, ⌘⇧↩). No screen-capture capability existed in this codebase
//! before — the pre-existing "system audio capture" is a CoreAudio *audio*
//! tap and shares nothing with this.
//!
//! **This is a seam on purpose.** `capture_primary_display` is the single
//! function that knows *how* a frame is grabbed. The `xcap` crate was picked
//! over shelling out to macOS's `/usr/sbin/screencapture` because a
//! subprocess costs a real ~150-400ms on a path the owner explicitly asked
//! to be fast, and over hand-binding ScreenCaptureKit through `objc2`
//! because that is multi-day work for a first screenshot feature (this
//! project already carries one hard-won objc2/CoreAudio integration and
//! doesn't need a second yet). If `xcap` ever needs replacing, this one
//! function's body is the whole change.
//!
//! **The blank-frame check is not defensive padding — it is the permission
//! error.** macOS, when Screen Recording has not been granted, does not
//! return an error from the underlying CoreGraphics path; it returns a
//! *black image*. Without [`looks_blank`], a denied permission would spend a
//! real API call asking the AI to analyze a black rectangle and then show
//! the owner a confident answer about nothing.
//!
//! No `Info.plist` key is needed: macOS has no `NSScreenCaptureUsageDescription`
//! — the TCC prompt is issued against the bundle identity directly, which is
//! why `scripts/build-app.sh`'s stable-signing support matters here.

pub mod cache;
pub mod encode;

/// A captured frame as raw RGBA8, the shape `encode` consumes. Deliberately
/// not an `xcap` or `image` type, so the seam above is real and swapping the
/// capture backend doesn't ripple into the encoder.
#[derive(Debug, Clone)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// True when every sampled pixel is within `tolerance` of the first one —
/// i.e. the frame is a flat, featureless rectangle. On macOS this is what a
/// missing Screen Recording permission actually looks like.
///
/// Samples on a stride rather than reading every pixel: a 6K display is ~50
/// million bytes and this runs on the latency path. A real screenshot has
/// visible variation within any few thousand pixels; a flat fill has none
/// anywhere, so a sparse sample is as conclusive as a full scan and
/// thousands of times cheaper.
pub fn looks_blank(img: &CapturedImage) -> bool {
    if img.rgba.len() < 4 {
        return true;
    }
    const TOLERANCE: i16 = 6;
    // Every 997th pixel — a prime stride, so it can't accidentally align
    // with a repeating pattern in the image (a striped background sampled
    // on a matching stride would read as flat).
    const PIXEL_STRIDE: usize = 997;

    let first = &img.rgba[0..3];
    let mut offset = 0usize;
    while offset + 3 <= img.rgba.len() {
        let px = &img.rgba[offset..offset + 3];
        for c in 0..3 {
            if (px[c] as i16 - first[c] as i16).abs() > TOLERANCE {
                return false;
            }
        }
        offset += PIXEL_STRIDE * 4;
    }
    true
}

/// The plain-language message for the blank-frame case. Kept next to the
/// check so the two never drift, and phrased per `CLAUDE.md`'s standing rule
/// — it names the exact place to go, not the underlying API that failed.
pub const BLANK_FRAME_MESSAGE: &str = "Aside can't see your screen yet. Open System Settings → Privacy & Security → Screen & System Audio Recording and turn Aside on, then try again.";

/// Grabs the primary display. Real I/O — the only impure function here.
#[cfg(not(test))]
pub fn capture_primary_display() -> Result<CapturedImage, String> {
    use xcap::Monitor;

    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" { fn CGPreflightScreenCaptureAccess() -> bool; }
        // A denied capture can contain wallpaper rather than a black frame;
        // pixel variation alone cannot tell whether window content is visible.
        if !unsafe { CGPreflightScreenCaptureAccess() } {
            return Err(BLANK_FRAME_MESSAGE.to_string());
        }
    }

    let monitors = Monitor::all().map_err(|e| format!("could not list displays: {e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .ok_or_else(|| "no primary display found".to_string())?;

    let frame = monitor
        .capture_image()
        .map_err(|e| format!("could not capture the screen: {e}"))?;
    let (width, height) = (frame.width(), frame.height());
    let captured = CapturedImage {
        width,
        height,
        rgba: frame.into_raw(),
    };

    if looks_blank(&captured) {
        return Err(BLANK_FRAME_MESSAGE.to_string());
    }
    Ok(captured)
}

/// Under `cfg(test)` there is no display to capture and no permission to
/// grant, so this reports that plainly instead of failing in some
/// environment-dependent way. Every testable rule in this module
/// ([`looks_blank`], everything in [`encode`]) is pure and covered against
/// synthetic images; the real grab is exercised by the manual check
/// recorded in `docs/evidence.md`, the same honest split this project
/// already applies to live audio capture.
#[cfg(test)]
pub fn capture_primary_display() -> Result<CapturedImage, String> {
    Err("screen capture is not exercised in unit tests — see this module's doc comment".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> CapturedImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        CapturedImage {
            width,
            height,
            rgba,
        }
    }

    fn gradient(width: u32, height: u32) -> CapturedImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 128, 255]);
            }
        }
        CapturedImage {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn an_all_black_frame_is_recognized_as_blank() {
        // The exact thing macOS returns when Screen Recording is denied.
        assert!(looks_blank(&solid(1920, 1080, [0, 0, 0])));
    }

    #[test]
    fn an_all_white_frame_is_also_blank() {
        // Not just black: any flat fill means we saw nothing useful.
        assert!(looks_blank(&solid(800, 600, [255, 255, 255])));
    }

    #[test]
    fn a_real_looking_frame_with_content_is_not_blank() {
        assert!(!looks_blank(&gradient(1920, 1080)));
    }

    #[test]
    fn a_mostly_flat_frame_with_a_small_region_of_content_is_not_blank() {
        // A real screenshot of a dark IDE is *nearly* flat. It must not be
        // mistaken for a permission failure.
        let mut img = solid(2000, 1200, [18, 18, 20]);
        // Paint a bright band well into the image, wide enough that the
        // sampling stride cannot step over all of it.
        let start = 4 * 2000 * 400;
        for i in 0..(4 * 2000 * 40) {
            img.rgba[start + i] = if i % 4 == 3 { 255 } else { 230 };
        }
        assert!(
            !looks_blank(&img),
            "a dark screenshot with content must not read as blank"
        );
    }

    #[test]
    fn a_tiny_or_empty_buffer_is_blank_rather_than_a_panic() {
        assert!(looks_blank(&CapturedImage {
            width: 0,
            height: 0,
            rgba: Vec::new()
        }));
        assert!(looks_blank(&CapturedImage {
            width: 1,
            height: 1,
            rgba: vec![1, 2]
        }));
    }

    #[test]
    fn the_blank_frame_message_names_where_to_fix_it_in_plain_language() {
        assert!(BLANK_FRAME_MESSAGE.contains("System Settings"));
        assert!(BLANK_FRAME_MESSAGE.contains("Screen & System Audio Recording"));
        // Plain-language rule: no API/permission-system vocabulary.
        assert!(!BLANK_FRAME_MESSAGE.contains("TCC"));
        assert!(!BLANK_FRAME_MESSAGE.contains("CGDisplay"));
    }
}
