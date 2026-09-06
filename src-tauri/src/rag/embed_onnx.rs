//! Local MiniLM sentence embeddings via `ort` (`status.md` 7.1(b) / 7.4).
//!
//! `Xenova/all-MiniLM-L6-v2` (a `BertModel` ONNX export, 384-dim output,
//! confirmed by inspecting the real downloaded graph's embedded input/output
//! names this session — `input_ids`/`attention_mask`/`token_type_ids` in,
//! `last_hidden_state` out). This is the exact model
//! `references/Repo1-Natively` vendors for its own local embedding path
//! (`resources/models/Xenova/all-MiniLM-L6-v2/`) — same model, independently
//! adopted after this phase's own research pointed at the same MiniLM class
//! `status.md` 7.1(b) named, not copied from their code (different stack:
//! their loader is `@huggingface/transformers`/`onnxruntime-node`, this one
//! is `ort` + the `tokenizers` crate, both already proven in this codebase
//! by `stt/parakeet_local.rs`).
//!
//! **Must never run on a real-time thread.** Inference is a blocking ONNX
//! Runtime call — every invocation goes through
//! `tauri::async_runtime::spawn_blocking`, exactly `parakeet_local.rs`'s
//! rule for the same reason.
//!
//! Sentence vector = mean-pooling `last_hidden_state` over real (non-padding)
//! tokens, weighted by the attention mask, then L2-normalized — the
//! standard `sentence-transformers` recipe for this exact model family, and
//! what makes its output directly comparable by cosine similarity.

use std::path::Path;
use std::sync::{Arc, Mutex};

use ndarray::Array2;
use ort::session::Session;
use ort::value::Value;
use tokenizers::Tokenizer;

use super::embed::{Embedder, EmbedderKind};

pub const DIMENSIONS: usize = 384;

/// BERT's own hard limit (`max_position_embeddings` in the model's
/// `config.json`) — an 80-word chunk tokenizes to well under this, but a
/// pasted document chunk (Phase 8, future) might not, so truncation is
/// enabled rather than left to fail loudly on an oversized input.
const MAX_SEQ_LEN: usize = 512;

pub struct OnnxEmbedder {
    session: Arc<Mutex<Session>>,
    tokenizer: Arc<Tokenizer>,
}

impl OnnxEmbedder {
    /// Loads the ONNX model + tokenizer from `model_dir` (the folder
    /// `embed_onnx_cache::ensure_downloaded` fills — `model.onnx` +
    /// `tokenizer.json`). Blocking (model load + graph compilation) — call
    /// via `spawn_blocking`, matching `ParakeetLocalEngine::load`'s own
    /// contract.
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .map_err(|e| format!("failed to create ONNX session builder: {e}"))?
            .commit_from_file(model_dir.join("model.onnx"))
            .map_err(|e| format!("failed to load MiniLM model.onnx: {e}"))?;

        let mut tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| format!("failed to load MiniLM tokenizer.json: {e}"))?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: MAX_SEQ_LEN,
                ..Default::default()
            }))
            .map_err(|e| format!("failed to configure tokenizer truncation: {e}"))?;

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(tokenizer),
        })
    }

    fn embed_blocking(
        session: &Mutex<Session>,
        tokenizer: &Tokenizer,
        text: &str,
    ) -> Result<Vec<f32>, String> {
        let encoding = tokenizer
            .encode(text, true)
            .map_err(|e| format!("tokenization failed: {e}"))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let seq_len = ids.len();
        if seq_len == 0 {
            return Ok(vec![0.0; DIMENSIONS]);
        }
        let type_ids: Vec<i64> = vec![0; seq_len];

        let input_ids = Array2::from_shape_vec((1, seq_len), ids)
            .map_err(|e| format!("failed to shape input_ids: {e}"))?;
        let attention_mask = Array2::from_shape_vec((1, seq_len), mask.clone())
            .map_err(|e| format!("failed to shape attention_mask: {e}"))?;
        let token_type_ids = Array2::from_shape_vec((1, seq_len), type_ids)
            .map_err(|e| format!("failed to shape token_type_ids: {e}"))?;

        let input_ids_value =
            Value::from_array(input_ids).map_err(|e| format!("input_ids value: {e}"))?;
        let attention_mask_value =
            Value::from_array(attention_mask).map_err(|e| format!("attention_mask value: {e}"))?;
        let token_type_ids_value =
            Value::from_array(token_type_ids).map_err(|e| format!("token_type_ids value: {e}"))?;

        let mut session = session.lock().unwrap_or_else(|p| p.into_inner());
        let outputs = session
            .run(ort::inputs!(
                "input_ids" => input_ids_value,
                "attention_mask" => attention_mask_value,
                "token_type_ids" => token_type_ids_value,
            ))
            .map_err(|e| format!("MiniLM inference failed: {e}"))?;

        let (shape, data) = outputs["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("failed to extract last_hidden_state: {e}"))?;
        let dims = shape.as_ref();
        if dims.len() != 3 || dims[2] as usize != DIMENSIONS {
            return Err(format!("unexpected last_hidden_state shape: {dims:?}"));
        }
        let hidden = dims[2] as usize;

        Ok(mean_pool_and_normalize(data, seq_len, hidden, &mask))
    }
}

/// Mean-pools `data` (a flat `[1, seq_len, hidden]` tensor) over the
/// sequence dimension, counting only positions where `mask` is `1` (real
/// tokens, not padding), then L2-normalizes the result — the standard
/// `sentence-transformers` pooling recipe for BERT-family embedding models.
/// A pure function, no ONNX/tokenizer types, so it's directly unit-testable
/// against hand-built fixtures.
fn mean_pool_and_normalize(data: &[f32], seq_len: usize, hidden: usize, mask: &[i64]) -> Vec<f32> {
    let mut pooled = vec![0.0f32; hidden];
    let mut real_tokens = 0f32;
    for t in 0..seq_len {
        if mask.get(t).copied().unwrap_or(0) == 0 {
            continue;
        }
        real_tokens += 1.0;
        let row_start = t * hidden;
        for h in 0..hidden {
            pooled[h] += data[row_start + h];
        }
    }
    if real_tokens > 0.0 {
        for v in pooled.iter_mut() {
            *v /= real_tokens;
        }
    }
    let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in pooled.iter_mut() {
            *v /= norm;
        }
    }
    pooled
}

#[async_trait::async_trait]
impl Embedder for OnnxEmbedder {
    fn kind(&self) -> EmbedderKind {
        EmbedderKind::Onnx
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let session = Arc::clone(&self.session);
        let tokenizer = Arc::clone(&self.tokenizer);
        let text = text.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            Self::embed_blocking(&session, &tokenizer, &text)
        })
        .await
        .map_err(|e| format!("MiniLM embedding task panicked: {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pool_ignores_padding_positions() {
        // hidden=2, seq_len=3, only the first 2 tokens are real (mask [1,1,0]).
        // Padding's values are deliberately extreme (100.0) so the test
        // would fail loudly if padding leaked into the average.
        let data = vec![
            1.0, 2.0, // token 0
            3.0, 4.0, // token 1
            100.0, 100.0, // token 2 — padding, must be ignored
        ];
        let mask = vec![1, 1, 0];
        let pooled = mean_pool_and_normalize(&data, 3, 2, &mask);
        // Unnormalized mean would be [(1+3)/2, (2+4)/2] = [2, 3]; after
        // L2-normalizing, direction must match [2,3], not something pulled
        // toward the padding value.
        let raw_norm = (2f32 * 2.0 + 3.0 * 3.0).sqrt();
        let expected = [2.0 / raw_norm, 3.0 / raw_norm];
        assert!((pooled[0] - expected[0]).abs() < 1e-5);
        assert!((pooled[1] - expected[1]).abs() < 1e-5);
    }

    #[test]
    fn mean_pool_output_is_unit_normalized() {
        let data = vec![3.0, 4.0, 5.0, 6.0];
        let mask = vec![1, 1];
        let pooled = mean_pool_and_normalize(&data, 2, 2, &mask);
        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn all_padding_does_not_panic_or_nan() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let mask = vec![0, 0];
        let pooled = mean_pool_and_normalize(&data, 2, 2, &mask);
        assert!(pooled.iter().all(|v| !v.is_nan()));
        assert!(pooled.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn dimensions_report_the_real_minilm_hidden_size() {
        assert_eq!(DIMENSIONS, 384);
    }
}
