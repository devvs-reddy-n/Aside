//! "Fill fields from Job Post URL" (`future refs/live-helper-ui-spec.md`
//! §3.3) — one Tauri command: fetch a job-posting page, strip it down to
//! plain visible text, hand that back to the frontend to drop into
//! Company/Job Description. Deliberately minimal: no headless-browser
//! rendering, no per-site scraping rules, just a heuristic tag strip — real
//! enough to save a copy/paste, not a general web-scraping engine. No
//! command here ever starts a session; the frontend owns deciding what to
//! do with the returned text (spec: "Never auto-start a session").

use tauri::State;

use crate::llm::HttpClient;

/// Hard cap on how much page text is returned — a job posting page is never
/// legitimately this large as *visible* text; anything past this is almost
/// always boilerplate (nav chrome, footers, other job listings) that would
/// only bloat the Company/Job Description fields. Mirrors
/// `ingest::MAX_FILE_SIZE_BYTES`'s "protect the caller from an absurd
/// input" reasoning, sized for characters of extracted text instead of raw
/// file bytes.
const MAX_TEXT_CHARS: usize = 20_000;

/// Tags whose *content* must be dropped entirely, not just the tags
/// themselves — `<script>`/`<style>` bodies are never visible page text and
/// routinely contain braces/semicolons that would otherwise pollute the
/// output.
const DROP_CONTENT_TAGS: &[&str] = &["script", "style", "noscript", "svg", "head"];

/// Strips `html` down to a best-effort plain-text rendering of what a
/// reader would actually see: drops `<script>`/`<style>`/etc. bodies
/// entirely, turns every other tag into nothing (or a newline for
/// block-level tags, so paragraphs don't run together), decodes the small
/// set of HTML entities job postings actually use, and collapses runs of
/// blank lines. Pure and synchronous so it's unit-testable without a real
/// HTTP fetch.
pub fn strip_html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes = html.as_bytes();
    let mut i = 0usize;
    let lower = html.to_ascii_lowercase();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Find the tag name (skip a leading '/').
            let name_start = i + 1 + usize::from(bytes.get(i + 1) == Some(&b'/'));
            let name_end = html[name_start..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map(|off| name_start + off)
                .unwrap_or(html.len());
            let tag_name = lower.get(name_start..name_end).unwrap_or("");

            // Find the end of this tag ('>').
            let tag_close = html[i..]
                .find('>')
                .map(|off| i + off + 1)
                .unwrap_or(html.len());

            if DROP_CONTENT_TAGS.contains(&tag_name) && bytes.get(name_start - 1) != Some(&b'/') {
                // Skip forward to the matching closing tag, dropping
                // everything in between (the element's whole text content).
                let closing = format!("</{tag_name}");
                if let Some(rel) = lower[tag_close..].find(&closing) {
                    let close_end = lower[tag_close + rel..]
                        .find('>')
                        .map(|o| tag_close + rel + o + 1)
                        .unwrap_or(html.len());
                    i = close_end;
                    continue;
                }
                i = html.len();
                continue;
            }

            if matches!(
                tag_name,
                "p" | "br"
                    | "div"
                    | "li"
                    | "tr"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "section"
                    | "article"
            ) {
                out.push('\n');
            }
            i = tag_close;
            continue;
        }
        // Not a tag: copy the character through (decoding a few entities).
        if bytes[i] == b'&' {
            let rest = &html[i..];
            let (decoded, consumed) = decode_entity(rest);
            out.push_str(decoded.as_str());
            i += consumed;
            continue;
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }

    collapse_whitespace(&out)
}

/// Decodes the handful of HTML entities job-posting pages actually use.
/// Returns the decoded text and how many bytes of `rest` it consumed;
/// anything unrecognized is passed through as a literal `&` (consuming just
/// that one byte) rather than dropped, since silently eating characters
/// would corrupt real text.
fn decode_entity(rest: &str) -> (String, usize) {
    const ENTITIES: &[(&str, &str)] = &[
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        ("&nbsp;", " "),
    ];
    for (entity, replacement) in ENTITIES {
        if rest.starts_with(entity) {
            return (replacement.to_string(), entity.len());
        }
    }
    ("&".to_string(), 1)
}

/// Collapses runs of spaces/tabs down to one, and runs of 3+ newlines down
/// to two (one blank line between paragraphs, not a page of empty lines).
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0u32;
    for line in text.lines() {
        let trimmed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(&trimmed);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Fetches `url`, strips it to plain text, and returns up to
/// [`MAX_TEXT_CHARS`] characters of it. Never starts, edits, or saves a
/// session — the frontend decides what to do with the text (spec: "Never
/// auto-start a session"). Errors are returned as one short, readable line
/// so the Create screen can show it under the URL field per spec 3.3
/// ("If fetch fails, show a one-line error under the field").
#[tauri::command]
pub async fn fetch_job_post_text(
    http: State<'_, HttpClient>,
    url: String,
) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Paste a job post URL first.".to_string());
    }
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "That doesn't look like a valid URL.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http:// and https:// URLs are supported.".to_string());
    }

    let response = http
        .0
        .get(parsed)
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; AsideJobPostFetch/1.0)",
        )
        .send()
        .await
        .map_err(|e| format!("Could not reach that page: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "That page returned an error ({}).",
            response.status()
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|e| format!("Could not read that page: {e}"))?;
    let text = strip_html_to_text(&html);
    if text.trim().is_empty() {
        return Err("That page had no readable text to extract.".to_string());
    }
    Ok(text.chars().take(MAX_TEXT_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_keeps_visible_text() {
        let html = "<html><body><h1>Software Engineer</h1><p>Must know <b>Python</b> and SQL.</p></body></html>";
        let text = strip_html_to_text(html);
        assert!(text.contains("Software Engineer"));
        assert!(text.contains("Must know Python and SQL."));
        assert!(!text.contains('<'));
    }

    #[test]
    fn drops_script_and_style_content_entirely() {
        let html = "<style>.a{color:red}</style><script>var x = 1;</script><p>Real content</p>";
        let text = strip_html_to_text(html);
        assert!(!text.contains("color:red"));
        assert!(!text.contains("var x"));
        assert!(text.contains("Real content"));
    }

    #[test]
    fn decodes_common_entities() {
        let html = "<p>Salary: $100k &amp; up &mdash;&gt; apply &lt;here&gt;</p>";
        let text = strip_html_to_text(html);
        assert!(text.contains("$100k & up"));
        assert!(text.contains("> apply <here>"));
    }

    #[test]
    fn collapses_excess_blank_lines() {
        let html = "<p>Line one</p><p></p><p></p><p></p><p>Line two</p>";
        let text = strip_html_to_text(html);
        assert!(!text.contains("\n\n\n"), "got: {text:?}");
        assert!(text.contains("Line one"));
        assert!(text.contains("Line two"));
    }

    #[test]
    fn caps_output_length() {
        let huge = format!("<p>{}</p>", "word ".repeat(10_000));
        let text = strip_html_to_text(&huge);
        let capped: String = text.chars().take(MAX_TEXT_CHARS).collect();
        assert!(capped.chars().count() <= MAX_TEXT_CHARS);
    }
}
