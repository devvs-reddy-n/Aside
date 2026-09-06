//! Shared document text-extraction entry point (`status.md` 8.2) — one
//! [`extract_text`] function every ingestion path (today: the file-picker
//! Tauri command in `commands/documents.rs`) must call, so PDF and DOCX
//! extraction never grow two independently-drifting code paths.
//!
//! **This is stated explicitly as "Natively's lesson."**
//! `references/Repo1-Natively`'s `electron/services/SafeDocumentTextExtractor.ts`
//! carries this exact doc comment at its top: "Extracted from
//! electron/services/ModeReferenceFileIngestion.ts so a single file owns
//! the parser + safety contract" — i.e. Natively originally had its Modes
//! upload and its Profile Intelligence upload each grow their own document
//! parsing logic, which drifted, and had to be collapsed into one shared
//! module after the fact. Aside starts with the shared entry point instead
//! of repeating that mistake.
//!
//! Deliberately scoped to `.pdf` and `.docx` only — this phase's job is
//! resume/job-description ingestion, not Natively's broader "any reference
//! file" upload (`SAFE_DOCUMENT_EXTENSIONS` there also covers
//! `.txt`/`.md`/`.json`/`.csv`/`.xml`/`.html`/`.log`). Adding another format
//! later is one more `match` arm here plus one more `ingest/<format>.rs`,
//! not a redesign.

pub mod docx;
pub mod pdf;

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// What kind of document was uploaded — matches
/// `storage/migrations.rs`'s `documents.doc_type CHECK` constraint exactly.
/// Kept as a real enum (not a bare `String`) at this command-facing layer
/// for the same reason `stt::EngineKind`/`llm::LlmProviderKind` are: a
/// settings dropdown should be driven from `DocType::ALL`, never a
/// hand-copied list that can drift from what the schema actually accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    Resume,
    JobDescription,
    Other,
}

impl DocType {
    pub const ALL: [DocType; 3] = [DocType::Resume, DocType::JobDescription, DocType::Other];

    /// The exact string `storage::documents::insert_document`'s `doc_type`
    /// column — and its schema `CHECK` constraint — expects.
    pub fn db_value(self) -> &'static str {
        match self {
            DocType::Resume => "resume",
            DocType::JobDescription => "job_description",
            DocType::Other => "other",
        }
    }
}

/// Hard cap on accepted file size. A resume or job description is never
/// legitimately this large; a bigger "resume" selection is almost
/// certainly the wrong file. Mirrors `SafeDocumentTextExtractor.ts`'s own
/// size cap in spirit (it uses 50 MB across a broader format set); set
/// lower here since this phase's two formats are both compact,
/// mostly-text document formats.
pub const MAX_FILE_SIZE_BYTES: u64 = 25 * 1024 * 1024;

const MIN_PARSE_TIMEOUT_MS: u64 = 5_000;
const MAX_PARSE_TIMEOUT_MS: u64 = 60_000;
const PARSE_TIMEOUT_MS_PER_MB: u64 = 1_000;

/// Size-scaled parse timeout (`status.md` 8.3): a bigger file gets
/// proportionally more time budget, not one flat timeout applied to every
/// file size. Same real shape as `SafeDocumentTextExtractor.ts`'s own
/// `computeParseTimeoutMs` (a real minimum, a per-MB rate, capped at a real
/// maximum) — re-derived with this project's own constants rather than
/// copied, because Aside's real extractors run far faster at equivalent
/// file sizes than Natively's `pdf-parse`/`mammoth` (measured in
/// `examples/bench_ingest.rs`, numbers recorded in `docs/evidence.md`), so
/// Natively's 2000ms-per-MB budget (with a 30s floor) would be needlessly
/// generous for Aside's actual parse cost.
pub fn compute_parse_timeout_ms(file_size_bytes: u64) -> u64 {
    let size_mb = file_size_bytes as f64 / (1024.0 * 1024.0);
    let scaled = (size_mb * PARSE_TIMEOUT_MS_PER_MB as f64).round();
    if scaled < MIN_PARSE_TIMEOUT_MS as f64 {
        MIN_PARSE_TIMEOUT_MS
    } else if scaled > MAX_PARSE_TIMEOUT_MS as f64 {
        MAX_PARSE_TIMEOUT_MS
    } else {
        scaled as u64
    }
}

/// Extracts text from `path`, dispatching purely on file extension — the
/// one shared entry point every ingestion path must go through
/// (`status.md` 8.2). Rejects anything that isn't a real, present,
/// non-empty, size-capped, `.pdf`/`.docx` file with a clean `Err` before
/// ever touching a parser.
///
/// Runs the real parse (`pdf::extract` / `docx::extract`, both synchronous
/// with no built-in cancellation — true of both `pdf-extract` and
/// `docx-rs`, matching `SafeDocumentTextExtractor.ts`'s own documented
/// asymmetry for its DOCX/mammoth branch: "mammoth has no
/// cancellation/destroy surface at all ... it can only give up waiting on
/// it") on a worker thread and enforces the size-scaled timeout at this
/// call site over a channel. **Stated honestly:** a caller that hits the
/// timeout gets a clean error back immediately rather than hanging, but the
/// worker thread itself is not forcibly killed — it may keep running in the
/// background until it finishes on its own. Real-world resume/JD-sized
/// PDFs/DOCX files parse in tens of milliseconds (`docs/evidence.md`), so
/// this is a defensive backstop against a pathological file, not a path
/// expected to fire in practice.
pub fn extract_text(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("could not read file: {e}"))?;
    if !metadata.is_file() {
        return Err("selected path is not a regular file".to_string());
    }
    if metadata.len() == 0 {
        return Err("file is empty".to_string());
    }
    if metadata.len() > MAX_FILE_SIZE_BYTES {
        return Err(format!(
            "file is {} bytes, which exceeds the {} MB limit",
            metadata.len(),
            MAX_FILE_SIZE_BYTES / (1024 * 1024)
        ));
    }

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let path_owned = path.to_path_buf();
    let parse: Box<dyn FnOnce() -> Result<String, String> + Send> = match extension.as_deref() {
        Some("pdf") => Box::new(move || pdf::extract(&path_owned)),
        Some("docx") => Box::new(move || docx::extract(&path_owned)),
        other => {
            return Err(format!(
                "unsupported file type {:?} — only .pdf and .docx are supported",
                other.unwrap_or("(none)")
            ));
        }
    };

    let timeout_ms = compute_parse_timeout_ms(metadata.len());
    run_with_timeout(parse, timeout_ms)
}

/// Real timeout enforcement over a channel — see [`extract_text`]'s doc
/// comment for what this does and does not guarantee.
fn run_with_timeout(
    parse: Box<dyn FnOnce() -> Result<String, String> + Send>,
    timeout_ms: u64,
) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = parse();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("document parsing timed out after {timeout_ms}ms"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("document parser thread panicked".to_string())
        }
    }
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

    // ---- compute_parse_timeout_ms: real unit tests at different simulated
    // file sizes (status.md's explicit call-out) ----

    #[test]
    fn a_tiny_file_gets_the_minimum_timeout_not_a_near_zero_one() {
        assert_eq!(compute_parse_timeout_ms(0), MIN_PARSE_TIMEOUT_MS);
        assert_eq!(compute_parse_timeout_ms(1_024), MIN_PARSE_TIMEOUT_MS);
    }

    #[test]
    fn timeout_scales_up_with_file_size_between_the_floor_and_the_cap() {
        // 10 MB -> 10 * 1_000ms = 10_000ms, comfortably between the 5s floor
        // and the 60s cap, and strictly larger than a 1 MB file's timeout.
        let one_mb = compute_parse_timeout_ms(1 * 1024 * 1024);
        let ten_mb = compute_parse_timeout_ms(10 * 1024 * 1024);
        assert!(
            ten_mb > one_mb,
            "a bigger file must get a bigger budget, not a flat timeout"
        );
        assert_eq!(ten_mb, 10_000);
    }

    #[test]
    fn a_huge_file_is_clamped_to_the_maximum_timeout() {
        let hundred_mb = compute_parse_timeout_ms(100 * 1024 * 1024);
        assert_eq!(
            hundred_mb, MAX_PARSE_TIMEOUT_MS,
            "must clamp, not grow unbounded"
        );
    }

    #[test]
    fn timeout_is_monotonically_non_decreasing_across_a_size_sweep() {
        let mut previous = 0u64;
        for mb in [0, 1, 2, 5, 10, 20, 40, 80, 160] {
            let bytes = mb * 1024 * 1024;
            let timeout = compute_parse_timeout_ms(bytes);
            assert!(
                timeout >= previous,
                "timeout must never shrink as file size grows"
            );
            previous = timeout;
        }
    }

    // ---- extract_text dispatch + safety checks ----

    #[test]
    fn dispatches_pdf_extension_to_the_pdf_extractor() {
        let text = extract_text(&fixture("sample_resume.pdf")).unwrap();
        assert!(text.contains("Jordan Rivera"));
    }

    #[test]
    fn dispatches_docx_extension_to_the_docx_extractor() {
        let text = extract_text(&fixture("sample_resume.docx")).unwrap();
        assert!(text.contains("Jordan Rivera"));
    }

    #[test]
    fn an_unsupported_extension_is_a_clean_error_not_a_panic() {
        let path =
            std::env::temp_dir().join(format!("aside-ingest-test-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, "plain text").unwrap();
        let result = extract_text(&path);
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("a .txt file must be rejected, not silently parsed");
        assert!(err.contains("unsupported"));
    }

    #[test]
    fn a_missing_file_is_a_clean_error_not_a_panic() {
        let result = extract_text(&fixture("does_not_exist.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn an_empty_file_is_a_clean_error() {
        let path =
            std::env::temp_dir().join(format!("aside-ingest-empty-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, []).unwrap();
        let result = extract_text(&path);
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("an empty file must be rejected before ever reaching a parser");
        assert!(err.contains("empty"));
    }

    #[test]
    fn an_oversized_file_is_rejected_before_parsing() {
        // Doesn't actually allocate 25MB+1 of real content — a sparse-ish
        // write of the right length is enough to exercise the size check,
        // which reads only `metadata.len()`, never the file's bytes.
        let path =
            std::env::temp_dir().join(format!("aside-ingest-huge-{}.pdf", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_FILE_SIZE_BYTES + 1).unwrap();
        drop(file);
        let result = extract_text(&path);
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("a file over the size cap must be rejected");
        assert!(err.contains("MB limit"));
    }

    // ---- known OCR limitation (status.md 8.6): a clean error, not a
    // crash or garbage output ----

    #[test]
    fn a_scanned_image_only_pdf_via_extract_text_is_a_clean_error() {
        let result = extract_text(&fixture("sample_scanned_image_only.pdf"));
        let err = result.expect_err("an image-only PDF must not silently succeed with empty text");
        assert!(err.contains("scanned") || err.contains("image-only"));
    }
}
