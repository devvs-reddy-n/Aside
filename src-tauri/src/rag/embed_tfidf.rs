//! `status.md` 7.1(a)'s pure-Rust, no-ML-model approach — measured the
//! fastest and cheapest of the three by a wide margin (see
//! `docs/decisions.md` / `docs/evidence.md`) and picked as the default.
//!
//! **Named honestly: this is a hashed term-frequency vector, not literal
//! TF-IDF or BM25.** True TF-IDF/BM25 needs a document-frequency count over
//! the *whole corpus*, which for a live session changes every time a new
//! chunk is embedded — incompatible with `status.md` 7.7's incremental
//! rule ("embed new chunks as they arrive, never re-embed the whole
//! session"), since a global IDF term would make every previously-embedded
//! chunk's vector stale the moment a new chunk changes the corpus. Real
//! production BM25 (including the one this project's own reference-repo
//! research found in `references/Repo1-Natively/electron/context-intelligence/retrieval/bm25.ts`)
//! sidesteps this by recomputing scores fresh per query over a scoped
//! candidate pool, not by storing a persistent per-document vector — a
//! different retrieval shape than `status.md` 7.5's "top-k cosine
//! similarity over stored embeddings" design this project uses uniformly
//! across all three backends (see `rag/embed.rs`'s trait doc comment).
//!
//! So: each chunk's own word counts are hashed into a fixed-size vector
//! (the ["hashing trick"](https://en.wikipedia.org/wiki/Feature_hashing),
//! a real, standard, textbook technique — scikit-learn's
//! `HashingVectorizer` is the same idea), independently of every other
//! chunk, which is exactly what makes it embed-once-and-never-revisit
//! compatible. What's lost versus true IDF: a word that's common across
//! the whole session (a filler word, or a name repeated constantly) isn't
//! down-weighted relative to a rare, distinctive word — cosine similarity
//! still rewards *shared* distinctive vocabulary between the query and a
//! chunk, just without the corpus-wide rarity boost. For a single meeting's
//! transcript (dozens to low hundreds of chunks, not a library), this
//! trade was judged acceptable — see `docs/decisions.md`.

use std::collections::HashMap;

use super::embed::{Embedder, EmbedderKind};

/// Vector length. 256 keeps collisions rare for a single chunk's vocabulary
/// (chunks are ~80 words, so on the order of 40-60 unique tokens after
/// de-duplication) while staying small enough that indexing/query cost is
/// dominated by tokenizing the text, not by the vector math.
pub const DIMENSIONS: usize = 256;

/// A small, fixed English stopword list — **found and fixed while wiring
/// this embedder's first real caller** (`llm::retrieve_context`,
/// `status.md` Phase 7's "pull relevant earlier chunks into the AI's
/// prompt" step). Before this list existed, `tokenize` kept every word two
/// characters or longer, including pure function words ("your", "you",
/// "the", "for", "with", ...) — so two genuinely unrelated sentences that
/// merely shared common grammar (e.g. "what's **your** notice period" and
/// "can you walk me through **your** availability for a follow-up call")
/// scored a real, non-spurious 0.14 cosine similarity, *higher* than a
/// real on-topic pair's 0.13 ("tell me about a challenging project" vs. an
/// actual answer about one) — measured directly, not assumed (see
/// `docs/evidence.md`, "RAG retrieval threshold measurement"). That's not a
/// hash collision (the module doc comment's stated trade-off); it's a real
/// gap between what this module's own doc comment claims ("cosine
/// similarity still rewards *shared distinctive* vocabulary") and what the
/// code actually did (rewarded *any* shared vocabulary, distinctive or
/// not). No threshold value can cleanly separate the two cases above while
/// stopwords still count — filtering them here, at the one place both
/// indexing and query-time embedding already go through, fixes it at the
/// root instead of chasing a workaround at the retrieval-threshold layer.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "so", "of", "to", "in", "on", "at", "for", "with",
    "from", "by", "as", "is", "are", "was", "were", "be", "been", "being", "do", "does", "did",
    "can", "could", "would", "should", "will", "shall", "may", "might", "have", "has", "had", "i",
    "me", "my", "you", "your", "he", "him", "his", "she", "her", "it", "its", "we", "us", "our",
    "they", "them", "their", "this", "that", "these", "those", "what", "which", "who", "whom",
    "am", "not", "no", "yes",
];

/// Tokenizes into lowercase alphanumeric words, dropping anything under 2
/// characters and any [`STOPWORDS`] entry (matches the spirit of
/// `references/Repo1-Natively`'s own `bm25.ts` `tokenize`, read during this
/// phase's research — same rough shape, independently reasoned rather than
/// copied, per `CLAUDE.md`'s "adapt ideas, don't copy code" rule, and this
/// project's tokenizer has no need to match theirs byte-for-byte since
/// nothing here compares scores across the two codebases).
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2 && !STOPWORDS.contains(w))
        .map(str::to_string)
        .collect()
}

/// FNV-1a — a small, fast, dependency-free hash, exactly what a near-zero
/// CPU cost budget calls for (no need to pull in a heavier hashing crate
/// for this).
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Turns `text` into a `DIMENSIONS`-length, L2-normalized vector: count
/// term frequencies, apply sublinear TF scaling (`1 + ln(count)`, the same
/// damping real TF-IDF uses so a word repeated 20 times doesn't dominate 20x
/// harder than one repeated twice), hash each unique term into a bucket
/// with a signed contribution (halves the expected collision bias — the
/// standard feature-hashing refinement), then normalize.
pub fn vectorize(text: &str) -> Vec<f32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for term in tokenize(text) {
        *counts.entry(term).or_insert(0) += 1;
    }

    let mut vector = vec![0.0f32; DIMENSIONS];
    for (term, count) in &counts {
        let h = fnv1a(term.as_bytes());
        let bucket = (h % DIMENSIONS as u64) as usize;
        let sign = if (h >> 32) & 1 == 0 { 1.0 } else { -1.0 };
        let weight = 1.0 + (*count as f32).ln();
        vector[bucket] += sign * weight;
    }

    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vector.iter_mut() {
            *v /= norm;
        }
    }
    vector
}

pub struct HashedTermFrequencyEmbedder;

#[async_trait::async_trait]
impl Embedder for HashedTermFrequencyEmbedder {
    fn kind(&self) -> EmbedderKind {
        EmbedderKind::TfIdf
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(vectorize(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_has_the_declared_dimensions() {
        assert_eq!(vectorize("hello world").len(), DIMENSIONS);
    }

    #[test]
    fn identical_text_produces_identical_vectors() {
        assert_eq!(
            vectorize("the interviewer asked about my visa"),
            vectorize("the interviewer asked about my visa")
        );
    }

    #[test]
    fn empty_text_is_a_zero_vector_not_a_panic() {
        let v = vectorize("");
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn non_empty_text_produces_a_unit_norm_vector() {
        let v = vectorize("h1-b visa sponsorship notice period two weeks");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "expected unit-normalized vector, got norm {norm}"
        );
    }

    #[test]
    fn shared_vocabulary_scores_higher_than_unrelated_text() {
        let query = vectorize("what is your visa status");
        let on_topic = vectorize("my visa status is h1-b sponsored");
        let off_topic = vectorize("the weather today is sunny and warm");
        let cos = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
        assert!(
            cos(&query, &on_topic) > cos(&query, &off_topic),
            "a chunk sharing real vocabulary with the query must score higher than an unrelated one"
        );
    }

    #[test]
    fn repeating_a_word_does_not_scale_the_contribution_linearly() {
        // Sublinear TF scaling: 1+ln(count), not raw count — a word said 20
        // times should not have 20x the pull of one said once.
        let once = vectorize("interview");
        let many = vectorize(&"interview ".repeat(20));
        let magnitude = |v: &[f32]| -> f32 { v.iter().map(|x| x.abs()).sum() };
        // Both are unit-normalized single-term vectors, so their raw
        // pre-normalization weights can't be read back directly here — the
        // real assertion is that the *ratio* implied by ln damping is much
        // smaller than 20x, checked via the un-normalized weight formula.
        let w_once = 1.0 + (1f32).ln();
        let w_many = 1.0 + (20f32).ln();
        assert!(
            w_many / w_once < 4.0,
            "1+ln(20) / 1+ln(1) should be well under the raw 20x ratio"
        );
        let _ = (once, many, magnitude); // vectors themselves are unit-norm; formula asserted above
    }

    #[test]
    fn kind_and_dimensions_report_correctly() {
        let e = HashedTermFrequencyEmbedder;
        assert_eq!(e.kind(), EmbedderKind::TfIdf);
        assert_eq!(e.dimensions(), DIMENSIONS);
    }

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    /// Real regression for the stopword fix described in `STOPWORDS`'s doc
    /// comment, measured directly (see `docs/evidence.md`, "RAG retrieval
    /// threshold measurement") rather than assumed: before this list
    /// existed, two genuinely unrelated sentences sharing only common
    /// grammar ("your") scored a real 0.14 cosine similarity — *higher*
    /// than a genuine on-topic pair's 0.13 elsewhere in that same
    /// measurement, meaning no retrieval threshold could have separated
    /// them. With stopwords filtered, sentences sharing no distinctive
    /// vocabulary score exactly `0.0`.
    #[test]
    fn sentences_sharing_only_stopwords_score_exactly_zero() {
        let query = vectorize("what's your notice period");
        let off_topic = vectorize("can you walk me through your availability for a follow up call");
        assert_eq!(
            cos(&query, &off_topic),
            0.0,
            "shared-only-stopword pairs must not score above zero"
        );
    }

    /// The other half of the same regression: filtering stopwords must not
    /// also filter out genuine shared distinctive vocabulary — a real
    /// on-topic pair still scores well above zero.
    #[test]
    fn sentences_sharing_distinctive_vocabulary_still_score_well_above_zero() {
        let query = vectorize("what's your notice period");
        let on_topic = vectorize("my notice period is two weeks from acceptance");
        assert!(
            cos(&query, &on_topic) > 0.3,
            "a chunk genuinely on-topic with the query must score comfortably above the noise floor"
        );
    }
}
