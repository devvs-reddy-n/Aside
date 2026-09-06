//! Shrinking and JPEG-encoding a captured frame before it goes to the AI.
//!
//! **Why this exists rather than sending the raw frame.** A modern Mac
//! display is 3-6 megapixels; as raw PNG that is tens of megabytes to
//! upload before the model has seen anything, on a path the owner asked to
//! be fast. `references/Repo2-OpenCluely` sends full-resolution PNG with no
//! processing at all — that is a latency bug, not a model to copy.
//! `references/Repo1-Natively` resizes to a long edge of 1280-1536 and
//! JPEG-encodes at quality 78-90 depending on a named profile; the same
//! shape is adopted here with this project's own constants.
//!
//! **Default profile is the sharp one.** Aside's screenshots are usually
//! code or a shared document — text that the model has to actually read —
//! so the default keeps a 1536px long edge at quality 88 rather than
//! optimizing bytes at the cost of legibility. Natively's own profile for
//! this case carries the comment "code text needs clarity", and that
//! matches what this shortcut is for.
//!
//! EXIF is stripped implicitly: the JPEG is written from raw pixels, so
//! there is no metadata to carry over from anywhere.

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::{imageops, ExtendedColorType, ImageEncoder, RgbImage};

use super::CapturedImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageProfile {
    pub max_long_edge_px: u32,
    pub jpeg_quality: u8,
}

/// Text-legibility first — the default, because this shortcut exists to read
/// code and documents off the screen.
pub const TECHNICAL: ImageProfile = ImageProfile {
    max_long_edge_px: 1536,
    jpeg_quality: 88,
};

/// Smaller and faster, for a screen that's mostly pictures rather than text.
/// Not currently selectable in the UI; kept because the step-down ladder
/// below and any future setting both need a second point of reference.
#[allow(dead_code)]
pub const BALANCED: ImageProfile = ImageProfile {
    max_long_edge_px: 1280,
    jpeg_quality: 85,
};

/// Upper bound on what gets uploaded. Comfortably under provider limits
/// while leaving room for the base64 expansion (~4/3).
pub const MAX_BYTES: usize = 3_500_000;

/// Quality ladder walked when the first encode lands over [`MAX_BYTES`].
/// Reducing quality is preferred over reducing dimensions here: shrinking
/// further is what actually destroys small text, which is the one thing
/// this image is usually being sent to have read.
const QUALITY_LADDER: &[u8] = &[75, 65, 55];

/// Pure: the scaled dimensions for `(width, height)` under `max_long_edge`.
/// Never enlarges — a small window stays its own size rather than being
/// blown up into blur.
pub fn scaled_dimensions(width: u32, height: u32, max_long_edge: u32) -> (u32, u32) {
    let long_edge = width.max(height);
    if long_edge <= max_long_edge || long_edge == 0 {
        return (width, height);
    }
    let ratio = max_long_edge as f64 / long_edge as f64;
    // At least 1px on each axis — a 4000x1 strip must not scale to zero
    // height, which would make the encoder fail on a valid capture.
    let w = ((width as f64 * ratio).round() as u32).max(1);
    let h = ((height as f64 * ratio).round() as u32).max(1);
    (w, h)
}

fn to_rgb(img: &CapturedImage) -> Result<RgbImage, String> {
    let expected = (img.width as usize) * (img.height as usize) * 4;
    if img.rgba.len() < expected {
        return Err(format!(
            "captured frame is smaller than its own dimensions claim ({} bytes for {}x{})",
            img.rgba.len(),
            img.width,
            img.height
        ));
    }
    let mut rgb = Vec::with_capacity((img.width as usize) * (img.height as usize) * 3);
    for px in img
        .rgba
        .chunks_exact(4)
        .take((img.width as usize) * (img.height as usize))
    {
        rgb.extend_from_slice(&px[0..3]);
    }
    RgbImage::from_raw(img.width, img.height, rgb)
        .ok_or_else(|| "could not build an image from the captured frame".to_string())
}

fn encode_jpeg(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("could not encode the screenshot: {e}"))?;
    Ok(out)
}

/// Resize (only if needed), JPEG-encode, and base64 the result — the exact
/// string that goes into the provider's `data:image/jpeg;base64,…` URL.
pub fn to_jpeg_base64(img: &CapturedImage, profile: ImageProfile) -> Result<String, String> {
    let rgb = to_rgb(img)?;
    let (w, h) = scaled_dimensions(rgb.width(), rgb.height(), profile.max_long_edge_px);
    let resized = if (w, h) == (rgb.width(), rgb.height()) {
        rgb
    } else {
        // Lanczos3: the filter that best preserves small text under
        // downscaling, which is the whole point of this path.
        imageops::resize(&rgb, w, h, imageops::FilterType::Lanczos3)
    };

    let mut bytes = encode_jpeg(&resized, profile.jpeg_quality)?;
    for quality in QUALITY_LADDER {
        if bytes.len() <= MAX_BYTES {
            break;
        }
        bytes = encode_jpeg(&resized, *quality)?;
    }
    if bytes.len() > MAX_BYTES {
        return Err(
            "that screen is too detailed to send — try capturing a smaller window".to_string(),
        );
    }

    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic but genuinely varied image — no fixture files, and not a
    /// flat fill (which JPEG would compress to almost nothing, making the
    /// size tests meaningless).
    fn noisy(width: u32, height: u32) -> CapturedImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 7 + y * 13) % 256) as u8;
                let g = ((x * 31 + y * 3) % 256) as u8;
                let b = ((x ^ y) % 256) as u8;
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        CapturedImage {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn a_frame_already_under_the_cap_is_not_resized() {
        assert_eq!(scaled_dimensions(1200, 800, 1536), (1200, 800));
    }

    #[test]
    fn a_frame_over_the_cap_is_scaled_down_preserving_aspect_ratio() {
        let (w, h) = scaled_dimensions(3840, 2160, 1536);
        assert_eq!(w, 1536, "the long edge lands on the cap");
        assert_eq!(h, 864, "16:9 must stay 16:9");
    }

    #[test]
    fn a_tall_frame_scales_on_its_own_long_edge() {
        let (w, h) = scaled_dimensions(1000, 4000, 1536);
        assert_eq!(h, 1536);
        assert_eq!(w, 384);
    }

    #[test]
    fn a_small_frame_is_never_enlarged() {
        // Upscaling would add blur and bytes for no readable detail.
        assert_eq!(scaled_dimensions(400, 300, 1536), (400, 300));
    }

    #[test]
    fn an_extreme_aspect_ratio_never_scales_an_axis_to_zero() {
        let (w, h) = scaled_dimensions(8000, 1, 1536);
        assert!(w >= 1 && h >= 1, "got {w}x{h}");
    }

    #[test]
    fn encoding_produces_something_a_decoder_accepts_as_a_jpeg() {
        let b64 = to_jpeg_base64(&noisy(600, 400), TECHNICAL).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        // JPEG's magic bytes — proves this is a real image, not just
        // non-empty output.
        assert_eq!(
            &bytes[0..2],
            &[0xFF, 0xD8],
            "must start with a JPEG SOI marker"
        );
        let decoded = image::load_from_memory(&bytes).expect("must decode as a real image");
        assert_eq!(decoded.width(), 600);
        assert_eq!(decoded.height(), 400);
    }

    #[test]
    fn a_large_frame_is_actually_downscaled_before_encoding() {
        let b64 = to_jpeg_base64(&noisy(3000, 2000), TECHNICAL).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            decoded.width(),
            1536,
            "the long edge must land on the profile cap"
        );
        assert_eq!(decoded.height(), 1024);
    }

    #[test]
    fn lower_quality_really_does_produce_fewer_bytes() {
        // The size ladder is only meaningful if this holds.
        let rgb = to_rgb(&noisy(800, 600)).unwrap();
        let high = encode_jpeg(&rgb, 88).unwrap();
        let low = encode_jpeg(&rgb, 55).unwrap();
        assert!(
            low.len() < high.len(),
            "q55 ({}) should be smaller than q88 ({})",
            low.len(),
            high.len()
        );
    }

    #[test]
    fn the_encoded_result_stays_under_the_upload_cap() {
        let b64 = to_jpeg_base64(&noisy(4000, 2500), TECHNICAL).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        assert!(bytes.len() <= MAX_BYTES, "got {} bytes", bytes.len());
    }

    #[test]
    fn a_frame_whose_buffer_is_shorter_than_its_dimensions_fails_cleanly() {
        // Defensive: a truncated buffer must produce a readable error, not
        // a panic or a garbled image sent to the provider.
        let broken = CapturedImage {
            width: 100,
            height: 100,
            rgba: vec![0; 10],
        };
        let err = to_jpeg_base64(&broken, TECHNICAL).unwrap_err();
        assert!(err.contains("smaller than"), "got {err}");
    }

    #[test]
    fn the_default_profile_favors_text_legibility() {
        // Guards against someone tuning this down for upload speed and
        // silently making code screenshots unreadable to the model.
        assert!(TECHNICAL.max_long_edge_px >= 1536);
        assert!(TECHNICAL.jpeg_quality >= 85);
    }
}
