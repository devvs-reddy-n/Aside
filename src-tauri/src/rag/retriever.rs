//! Top-k cosine similarity + score threshold (`status.md` 7.5) — the one
//! retrieval algorithm every [`crate::rag::embed::Embedder`] backend shares.
//! Pure math, no I/O: callers hand it a query vector and a candidate list
//! (already loaded from `storage::chunks`), and get back the best matches.
//! This is also the plain-Rust correctness path `status.md` 7.6 requires to
//! always work, with or without `sqlite-vec` (see that module's doc
//! comment and `docs/decisions.md` for why `sqlite-vec` ended up not wired
//! in at all).

//! **Wired to a real caller.** `llm::retrieve_context` calls `top_k` at
//! answer time — the top-k chunks for the triggering question get spliced
//! into `llm::prompt::user_prompt` before the provider chain is asked for
//! an answer. See `llm/mod.rs`'s `retrieve_context`.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredChunk {
    pub chunk_id: Uuid,
    pub score: f32,
}

/// Cosine similarity between two vectors of equal length. Returns `0.0` for
/// a length mismatch or a zero-magnitude vector rather than panicking or
/// NaN-poisoning the caller's ranking — a chunk that can't be meaningfully
/// compared should sort last, not crash the query.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Ranks `candidates` against `query_vec` by cosine similarity, keeps only
/// scores `>= threshold`, and returns the top `k` — highest score first. A
/// low-relevance session (nothing on-topic yet) should come back empty
/// rather than forcing an irrelevant chunk into the prompt just to fill `k`
/// slots, which is what the threshold is for.
pub fn top_k(
    query_vec: &[f32],
    candidates: &[(Uuid, Vec<f32>)],
    k: usize,
    threshold: f32,
) -> Vec<ScoredChunk> {
    let mut scored: Vec<ScoredChunk> = candidates
        .iter()
        .map(|(id, vec)| ScoredChunk {
            chunk_id: *id,
            score: cosine_similarity(query_vec, vec),
        })
        .filter(|s| s.score >= threshold)
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_score_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_score_zero() {
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
    }

    #[test]
    fn opposite_vectors_score_negative_one() {
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn scale_invariant_same_direction_different_magnitude() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0]; // same direction, 2x magnitude
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mismatched_lengths_score_zero_not_panic() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0, 2.0, 3.0]), 0.0);
    }

    #[test]
    fn zero_vector_scores_zero_not_nan() {
        let score = cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]);
        assert_eq!(score, 0.0);
        assert!(!score.is_nan());
    }

    #[test]
    fn top_k_returns_highest_scores_first() {
        let query = vec![1.0, 0.0];
        let candidates = vec![
            (Uuid::from_u128(1), vec![0.0, 1.0]), // orthogonal, score 0
            (Uuid::from_u128(2), vec![1.0, 0.0]), // identical, score 1
            (Uuid::from_u128(3), vec![0.9, 0.1]), // close, high score
        ];
        let results = top_k(&query, &candidates, 10, 0.0);
        assert_eq!(results[0].chunk_id, Uuid::from_u128(2));
        assert_eq!(results[1].chunk_id, Uuid::from_u128(3));
        assert_eq!(results[2].chunk_id, Uuid::from_u128(1));
    }

    #[test]
    fn top_k_respects_the_k_limit() {
        let query = vec![1.0, 0.0];
        let candidates: Vec<(Uuid, Vec<f32>)> = (0..10)
            .map(|i| (Uuid::from_u128(i), vec![1.0, 0.0]))
            .collect();
        assert_eq!(top_k(&query, &candidates, 3, 0.0).len(), 3);
    }

    #[test]
    fn top_k_drops_anything_under_the_threshold() {
        let query = vec![1.0, 0.0];
        let candidates = vec![
            (Uuid::from_u128(1), vec![1.0, 0.0]), // score 1.0
            (Uuid::from_u128(2), vec![0.0, 1.0]), // score 0.0
        ];
        let results = top_k(&query, &candidates, 10, 0.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, Uuid::from_u128(1));
    }

    #[test]
    fn top_k_on_an_empty_candidate_list_is_an_empty_vec_not_an_error() {
        let results = top_k(&[1.0, 0.0], &[], 10, 0.0);
        assert!(results.is_empty());
    }
}
