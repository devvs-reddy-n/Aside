//! Planned spike (see the approved plan): confirm `xcap` actually returns a
//! real, non-black frame on this machine before the feature depends on it.
//! Not product code — run with `cargo run --example spike_capture`.
fn main() {
    match xcap::Monitor::all() {
        Err(e) => println!("FAIL: could not list displays: {e}"),
        Ok(monitors) => {
            println!("displays found: {}", monitors.len());
            for m in monitors {
                let primary = m.is_primary().unwrap_or(false);
                match m.capture_image() {
                    Ok(img) => {
                        let (w, h) = (img.width(), img.height());
                        let raw = img.into_raw();
                        let distinct: std::collections::HashSet<[u8; 3]> = raw
                            .chunks_exact(4)
                            .step_by(1013)
                            .map(|p| [p[0], p[1], p[2]])
                            .collect();
                        println!(
                            "  primary={primary} {w}x{h} bytes={} distinct_sampled_colors={}",
                            raw.len(),
                            distinct.len()
                        );
                        if distinct.len() <= 1 {
                            println!(
                                "  ^^ BLANK — Screen Recording permission is probably not granted"
                            );
                        }
                    }
                    Err(e) => println!("  primary={primary} capture failed: {e}"),
                }
            }
        }
    }
}
