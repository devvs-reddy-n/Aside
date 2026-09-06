//! PDF text extraction (`status.md` 8.3) — the `.pdf` half of
//! `ingest::extract_text`'s dispatch.
//!
//! Backed by `pdf-extract`, chosen over `lopdf::Document::extract_text`
//! after a real benchmark against real PDF fixtures
//! (`examples/bench_ingest.rs`, numbers recorded in `docs/evidence.md`):
//! `lopdf`'s bundled extractor is 2-4x faster but splits hyphenated words
//! and bullet glyphs across separate lines (e.g. `"cross-functional"` comes
//! back as three lines, `"cross"` / `"-"` / `"functional"`), which would
//! corrupt this project's sentence-boundary chunker (`rag::chunker`) and
//! its regex fact extractor (`facts::extract`). `pdf-extract` produced
//! clean, correctly-joined prose on the same real files.
//!
//! **Known, deliberate limitation, stated honestly (`status.md` 8.6):
//! scanned/image-only PDFs produce no text.** `pdf-extract` reads a PDF's
//! real text objects; a scanned page's content is a picture, with no text
//! objects to read. This returns a specific, clean error rather than
//! silently succeeding with an empty string a caller might mistake for "the
//! document really was blank." OCR is out of scope for this phase.

use std::path::Path;

pub fn extract(path: &Path) -> Result<String, String> {
    let text = pdf_extract::extract_text(path).map_err(|e| format!("failed to parse PDF: {e}"))?;
    if text.trim().is_empty() {
        return Err(
            "PDF parsed but contains no extractable text — likely a scanned/image-only PDF, \
             which is not supported (OCR is out of scope)"
                .to_string(),
        );
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ingest")
            .join(name)
    }

    /// Real PDF, real known text content (built with `python-docx`'s
    /// sibling `reportlab`, three `drawString` calls at known coordinates)
    /// — confirms the extractor gets that exact text back, not a
    /// close-enough approximation.
    #[test]
    fn extracts_the_exact_known_text_from_a_real_pdf() {
        let text = extract(&fixture("sample_resume.pdf")).unwrap();
        assert!(text.contains("Aside Test Fixture Resume"));
        assert!(text.contains("Jordan Rivera - Software Engineer"));
        assert!(text.contains("5 years of Python experience"));
    }

    #[test]
    fn a_scanned_image_only_pdf_is_a_clean_error_not_empty_success_or_a_crash() {
        let result = extract(&fixture("sample_scanned_image_only.pdf"));
        let err = result.expect_err("must not silently return empty text for a scanned PDF");
        assert!(err.contains("scanned") || err.contains("image-only"));
    }

    #[test]
    fn a_file_that_is_not_actually_a_pdf_is_a_clean_error_not_a_panic() {
        // A real .docx (a zip archive) has a completely different binary
        // shape than a PDF — this exercises the parser's own format
        // rejection, not just a missing-file path.
        let result = extract(&fixture("sample_resume.docx"));
        assert!(result.is_err());
    }

    #[test]
    fn a_missing_file_is_a_clean_error() {
        assert!(extract(&fixture("does_not_exist.pdf")).is_err());
    }
}
