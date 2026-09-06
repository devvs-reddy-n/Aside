//! Splits transcript text into ~80-word chunks for embedding (`status.md`
//! 7.3).
//!
//! **Why 80 words, and why sentence boundaries, not a hard word cut.**
//! `status.md` calls for "AssistAI's shape, right-sized" — AssistAI isn't
//! one of this project's two vendored reference repos (`references/`), so
//! this is this project's own judgment call, stated honestly rather than
//! attributed to a repo that was never read: 80 words is roughly one to
//! three spoken sentences — small enough that a chunk stays about *one*
//! topic (so a cosine-similarity match against it is actually relevant to
//! the question that matched it), large enough that indexing a whole
//! session doesn't produce hundreds of one-line fragments that all score
//! similarly and drown out the real match. Cutting mid-sentence would
//! occasionally sever the exact clause a later question needs ("...my visa
//! status is" | "H1-B, sponsored by..."), so this always cuts at a sentence
//! boundary, allowing a chunk to run a little over 80 words rather than
//! split a sentence in half.
//!
//! Two entry points: [`chunk_text`] (whole text at once, e.g. a future
//! Phase 8 document) and [`StreamingChunker`] (accumulates text across
//! multiple `push` calls, only ever handing back *complete* ~80-word
//! chunks and keeping the trailing partial sentence(s) buffered — this is
//! what makes incremental indexing possible, `status.md` 7.7: each new
//! final transcript segment is pushed in, and a chunk is only embedded once
//! it's actually full, never re-cut or re-embedded).

/// Target chunk size in words. A chunk is allowed to run over this when a
/// single sentence alone exceeds it (never split mid-sentence) — see the
/// module doc comment.
pub const TARGET_CHUNK_WORDS: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub text: String,
    pub word_count: usize,
    /// True when this chunk was cut because it *reached* the target size —
    /// false when it was handed back only because the input ran out (the
    /// trailing remainder of a document, or a still-accumulating streaming
    /// buffer). [`StreamingChunker`] uses this to decide whether the last
    /// chunk `chunk_text` returns is ready to emit or should stay buffered.
    pub is_complete: bool,
}

/// Splits `text` into sentences on `.`/`!`/`?` followed by whitespace or
/// end-of-string (repeated punctuation like `?!` or `...` counts as one
/// boundary). Deliberately not a regex with a lookbehind — the `regex`
/// crate doesn't support those — so this is a plain byte scan. Safe on
/// UTF-8: the only bytes it ever tests against are the single-byte ASCII
/// values `.`/`!`/`?`/whitespace, which can never appear as a continuation
/// byte of a multi-byte UTF-8 sequence, so every slice point this function
/// computes lands on a real character boundary.
fn split_sentences(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'.' || c == b'!' || c == b'?' {
            let mut j = i + 1;
            while j < bytes.len() && matches!(bytes[j], b'.' | b'!' | b'?') {
                j += 1;
            }
            if j >= bytes.len() || bytes[j].is_ascii_whitespace() {
                let s = text[start..j].trim();
                if !s.is_empty() {
                    sentences.push(s);
                }
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        sentences.push(rest);
    }
    sentences
}

fn finish_chunk(sentences: &[&str], word_count: usize) -> Chunk {
    Chunk {
        text: sentences.join(" "),
        word_count,
        is_complete: word_count >= TARGET_CHUNK_WORDS,
    }
}

/// Splits `text` into ~80-word chunks, greedily packing whole sentences and
/// cutting once the running total reaches [`TARGET_CHUNK_WORDS`]. The final
/// chunk (whatever's left after the last full sentence) is always included
/// even if it's short — callers that care whether that's "actually a full
/// chunk" should check the last element's `is_complete`.
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let sentences = split_sentences(text);
    let mut chunks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_words = 0usize;

    for sentence in sentences {
        let sentence_words = sentence.split_whitespace().count();
        if current_words > 0 && current_words + sentence_words > TARGET_CHUNK_WORDS {
            chunks.push(finish_chunk(&current, current_words));
            current.clear();
            current_words = 0;
        }
        current.push(sentence);
        current_words += sentence_words;
        if current_words >= TARGET_CHUNK_WORDS {
            chunks.push(finish_chunk(&current, current_words));
            current.clear();
            current_words = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(finish_chunk(&current, current_words));
    }
    chunks
}

/// Accumulates transcript text across multiple final segments and only ever
/// hands back chunks once they're actually full — the piece that makes
/// `status.md` 7.7 ("embed new chunks as they arrive, never re-embed the
/// whole session") possible. Never re-scans text that's already been
/// emitted: each `push` re-chunks only the still-pending buffer (bounded to
/// roughly one chunk's worth of text at a time), not the session so far.
#[derive(Debug, Default)]
pub struct StreamingChunker {
    pending: String,
}

impl StreamingChunker {
    /// Feeds one more final transcript segment's text in. Returns zero or
    /// more chunks that are now complete; any trailing incomplete sentence
    /// fragment stays buffered for the next call.
    pub fn push(&mut self, text: &str) -> Vec<Chunk> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if !self.pending.is_empty() {
            self.pending.push(' ');
        }
        self.pending.push_str(text);

        let mut all = chunk_text(&self.pending);
        match all.last() {
            Some(last) if !last.is_complete => {
                let tail = all.pop().expect("just checked last()");
                self.pending = tail.text;
                all
            }
            _ => {
                self.pending.clear();
                all
            }
        }
    }

    /// Emits whatever's left in the buffer as a final chunk (marked
    /// complete regardless of size) — call this when a session ends so the
    /// last partial thought isn't silently dropped.
    pub fn flush(&mut self) -> Option<Chunk> {
        let text = std::mem::take(&mut self.pending);
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let word_count = text.split_whitespace().count();
        Some(Chunk {
            text: text.to_string(),
            word_count,
            is_complete: true,
        })
    }

    #[cfg(test)]
    pub fn pending_word_count(&self) -> usize {
        self.pending.split_whitespace().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(n: usize, prefix: &str) -> String {
        (0..n)
            .map(|i| format!("{prefix}{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn short_text_is_one_incomplete_chunk() {
        let chunks = chunk_text("This is a short sentence.");
        assert_eq!(chunks.len(), 1);
        assert!(!chunks[0].is_complete);
        assert_eq!(chunks[0].word_count, 5);
    }

    #[test]
    fn cuts_at_a_sentence_boundary_once_target_is_reached() {
        // 5 sentences of 20 words each = 100 words total. The cut must land
        // after a whole sentence, at or just past 80, never mid-sentence.
        let mut text = String::new();
        for s in 0..5 {
            text.push_str(&words(20, &format!("s{s}w")));
            text.push_str(". ");
        }
        let chunks = chunk_text(&text);
        assert!(
            chunks.len() >= 2,
            "100 words should not fit in one 80-word-target chunk"
        );
        assert!(chunks[0].is_complete);
        assert!(chunks[0].word_count >= TARGET_CHUNK_WORDS);
        // No chunk boundary lands inside a sentence — every chunk's text
        // ends with sentence punctuation or is the final trailing chunk.
        for c in &chunks[..chunks.len() - 1] {
            assert!(
                c.text.ends_with('.'),
                "non-final chunk must end at a real sentence boundary, got: {:?}",
                c.text
            );
        }
    }

    #[test]
    fn a_single_sentence_longer_than_target_is_never_split() {
        let long_sentence = format!("{}.", words(200, "w"));
        let chunks = chunk_text(&long_sentence);
        assert_eq!(
            chunks.len(),
            1,
            "one long sentence must stay one chunk, never split mid-sentence"
        );
        assert_eq!(chunks[0].word_count, 200);
        assert!(chunks[0].is_complete);
    }

    #[test]
    fn empty_text_produces_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   ").is_empty());
    }

    #[test]
    fn repeated_punctuation_counts_as_one_boundary() {
        let chunks = chunk_text("Wait, really?! Yes, truly.");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "Wait, really?! Yes, truly.");
    }

    #[test]
    fn streaming_chunker_buffers_until_a_chunk_is_actually_full() {
        let mut sc = StreamingChunker::default();
        // Three small segments, well under 80 words combined — nothing
        // should be emitted yet, exactly what makes incremental indexing
        // cheap (no chunk embedded until it's genuinely ready).
        assert!(sc
            .push("The interviewer asked about my background.")
            .is_empty());
        assert!(sc.push("I explained my last role in detail.").is_empty());
        assert!(sc
            .push("Then we moved on to technical questions.")
            .is_empty());
        assert!(sc.pending_word_count() > 0);
    }

    #[test]
    fn streaming_chunker_emits_exactly_one_chunk_once_target_is_crossed() {
        let mut sc = StreamingChunker::default();
        let mut emitted = Vec::new();
        // Feed 20-word sentences one at a time; the 5th push crosses 80.
        for s in 0..5 {
            let seg = format!("{}.", words(20, &format!("seg{s}w")));
            emitted.extend(sc.push(&seg));
        }
        assert_eq!(
            emitted.len(),
            1,
            "exactly one chunk should have been cut by the time 100 words arrived"
        );
        assert!(emitted[0].is_complete);
        assert!(emitted[0].word_count >= TARGET_CHUNK_WORDS);
    }

    #[test]
    fn streaming_chunker_never_re_emits_a_chunk_already_returned() {
        // The regression this guards: incremental indexing must never
        // re-embed text that was already cut into a chunk (status.md 7.7).
        let mut sc = StreamingChunker::default();
        let mut all_emitted_text = Vec::new();
        for s in 0..10 {
            let seg = format!("{}.", words(15, &format!("t{s}w")));
            for c in sc.push(&seg) {
                all_emitted_text.push(c.text);
            }
        }
        if let Some(last) = sc.flush() {
            all_emitted_text.push(last.text);
        }
        let mut seen = std::collections::HashSet::new();
        for text in &all_emitted_text {
            assert!(
                seen.insert(text.clone()),
                "chunk text emitted twice: {text:?}"
            );
        }
    }

    #[test]
    fn flush_returns_none_when_nothing_is_pending() {
        let mut sc = StreamingChunker::default();
        assert!(sc.flush().is_none());
    }

    #[test]
    fn flush_returns_the_trailing_partial_chunk_marked_complete() {
        let mut sc = StreamingChunker::default();
        sc.push("A short trailing thought.");
        let flushed = sc.flush().expect("pending text should flush");
        assert!(
            flushed.is_complete,
            "flush must mark the tail complete so it actually gets embedded"
        );
        assert_eq!(flushed.word_count, 4);
        assert!(
            sc.flush().is_none(),
            "flush must drain the buffer, not leave it re-flushable"
        );
    }
}
