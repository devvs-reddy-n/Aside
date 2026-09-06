//! `status.md` 8.1's real benchmark — measures, on real fixture files (not
//! synthetic placeholders), `pdf-extract` vs `lopdf`'s bundled
//! `extract_text` for PDF text extraction, and `docx-rs` for the Word-doc
//! path. Kept permanently, mirroring `examples/bench_rag.rs`'s precedent of
//! a real research script that stays in the tree as the record of how the
//! library choice was actually made, not a throwaway deleted after the
//! decision.
//!
//! Run with:
//! ```text
//! cargo run --example bench_ingest -- <path-to.pdf> <path-to-larger.pdf> <path-to.docx>
//! ```
//! Real numbers from an actual run (real resume/job-description PDFs and a
//! real-shaped `.docx` built by `python-docx`, since `references/`'s own
//! `.docx` fixture turned out to be missing standard OOXML parts and
//! `docx-rs` correctly rejected it — see `docs/evidence.md`) are recorded in
//! `docs/evidence.md`, not estimated. **Result: `pdf-extract` was chosen.**
//! It is 2-4x slower than `lopdf::Document::extract_text` at this project's
//! real file sizes (tens of milliseconds either way, irrelevant at
//! once-per-upload scale) but produces markedly cleaner text — `lopdf`'s
//! extractor splits hyphenated words and bullet glyphs across separate
//! lines (`"cross"` / `"-"` / `"functional"` as three lines instead of
//! `"cross-functional"`), which would corrupt this project's sentence-
//! boundary chunker (`rag::chunker`) and its regex fact extractor
//! (`facts::extract`, e.g. a hyphen-broken `"problem-solving"` would no
//! longer match a phrasing pattern expecting it whole).

use std::time::Instant;

fn try_pdf_extract(path: &str) {
    let start = Instant::now();
    match pdf_extract::extract_text(path) {
        Ok(text) => {
            let elapsed = start.elapsed();
            println!(
                "  pdf-extract: OK in {:?}, {} chars, {} words",
                elapsed,
                text.len(),
                text.split_whitespace().count()
            );
            let preview: String = text
                .split_whitespace()
                .take(25)
                .collect::<Vec<_>>()
                .join(" ");
            println!("    preview: {preview:?}");
        }
        Err(e) => println!("  pdf-extract: ERROR {e}"),
    }
}

fn try_lopdf(path: &str) {
    let start = Instant::now();
    match lopdf::Document::load(path) {
        Ok(doc) => {
            // lopdf has no built-in "give me the text" API — it's a
            // structural PDF object model, not a text extractor. Its own
            // docs point at `doc.extract_text(&page_numbers)` (added as a
            // convenience wrapper in newer versions) as the closest thing.
            let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
            match doc.extract_text(&pages) {
                Ok(text) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  lopdf: OK in {:?}, {} chars, {} words, {} pages",
                        elapsed,
                        text.len(),
                        text.split_whitespace().count(),
                        pages.len()
                    );
                    let preview: String = text
                        .split_whitespace()
                        .take(25)
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("    preview: {preview:?}");
                }
                Err(e) => println!("  lopdf: loaded but extract_text ERROR {e}"),
            }
        }
        Err(e) => println!("  lopdf: ERROR loading {e}"),
    }
}

fn try_docx_rs(path: &str) {
    let start = Instant::now();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("  docx-rs: ERROR reading file {e}");
            return;
        }
    };
    match docx_rs::read_docx(&bytes) {
        Ok(docx) => {
            let elapsed = start.elapsed();
            // docx-rs models the document as a tree of typed elements
            // (paragraphs -> runs -> text), not a flat string — this walks
            // it the same way the real product extractor will.
            let mut text = String::new();
            for child in &docx.document.children {
                if let docx_rs::DocumentChild::Paragraph(p) = child {
                    for run_child in &p.children {
                        if let docx_rs::ParagraphChild::Run(r) = run_child {
                            for rc in &r.children {
                                if let docx_rs::RunChild::Text(t) = rc {
                                    text.push_str(&t.text);
                                    text.push(' ');
                                }
                            }
                        }
                    }
                    text.push('\n');
                }
            }
            println!(
                "  docx-rs: OK in {:?}, {} chars, {} words",
                elapsed,
                text.len(),
                text.split_whitespace().count()
            );
            let preview: String = text
                .split_whitespace()
                .take(25)
                .collect::<Vec<_>>()
                .join(" ");
            println!("    preview: {preview:?}");
        }
        Err(e) => println!("  docx-rs: ERROR {e:?}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: bench_ingest <resume.pdf> <thesis.pdf> <resume.docx>");
        std::process::exit(1);
    }
    let resume_pdf = &args[1];
    let thesis_pdf = &args[2];
    let resume_docx = &args[3];

    println!("=== small real resume.pdf ({resume_pdf}) ===");
    try_pdf_extract(resume_pdf);
    try_lopdf(resume_pdf);

    println!("\n=== larger real thesis.pdf ({thesis_pdf}) ===");
    try_pdf_extract(thesis_pdf);
    try_lopdf(thesis_pdf);

    println!("\n=== real resume.docx ({resume_docx}) ===");
    try_docx_rs(resume_docx);
}
