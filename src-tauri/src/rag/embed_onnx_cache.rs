//! Downloads and caches the local MiniLM ONNX embedding model, mirroring
//! `stt/parakeet_cache.rs`'s established shape exactly (same
//! `.partial`-then-rename atomicity, same size-match completeness check,
//! same in-process concurrent-download guard) — this project's one existing
//! model-download precedent, not a new pattern invented for this phase.
//!
//! Model: `Xenova/all-MiniLM-L6-v2`, the fp32 ONNX export (90,387,606
//! bytes) — the exact file this phase's real benchmark (`docs/evidence.md`)
//! downloaded and measured on this machine. File names and byte sizes
//! match Hugging Face `Content-Length` on the stable `resolve/main` URLs
//! (2026-08-24). Never hardcode a redirected CDN URL — those are signed and
//! time-limited, same caution `parakeet_cache.rs` already states.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Folder name under `{app_data_dir}/models/` that holds this model's files.
pub const MODEL_DIR_NAME: &str = "all-minilm-l6-v2-onnx";

const HF_REPO: &str = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main";

pub const FILES: [ModelFile; 2] = [
    ModelFile {
        name: "onnx/model.onnx",
        expected_size_bytes: 90_387_606,
        dest_name: "model.onnx",
    },
    ModelFile {
        name: "tokenizer.json",
        expected_size_bytes: 711_661,
        dest_name: "tokenizer.json",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub expected_size_bytes: u64,
    /// Flattened destination filename — the HF repo nests `model.onnx`
    /// under `onnx/`, but there's no reason to mirror that directory shape
    /// locally when there's only ever one file there.
    pub dest_name: &'static str,
}

impl ModelFile {
    pub fn url(self) -> String {
        format!("{HF_REPO}/{}", self.name)
    }
}

/// `{models_root}/all-minilm-l6-v2-onnx`.
pub fn model_dir(models_root: &Path) -> PathBuf {
    models_root.join(MODEL_DIR_NAME)
}

fn file_is_complete(path: &Path, expected: u64) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() == expected)
        .unwrap_or(false)
}

/// True only when every required file is present at its expected byte size.
pub fn is_downloaded(models_root: &Path) -> bool {
    let dir = model_dir(models_root);
    FILES
        .iter()
        .all(|f| file_is_complete(&dir.join(f.dest_name), f.expected_size_bytes))
}

fn in_progress_downloads() -> &'static Mutex<HashSet<PathBuf>> {
    static LOCK: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(HashSet::new()))
}

struct DownloadGuard(PathBuf);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        in_progress_downloads()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

/// Downloads any missing files into `{models_root}/all-minilm-l6-v2-onnx`.
/// Each file is written to a `.partial` sibling and renamed only when the
/// byte count matches, so a crash never leaves a truncated file at the real
/// path that `is_downloaded` would trust.
pub async fn ensure_downloaded(
    client: &reqwest::Client,
    models_root: &Path,
) -> Result<PathBuf, String> {
    let dest_dir = model_dir(models_root);
    if is_downloaded(models_root) {
        return Ok(dest_dir);
    }

    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("could not create MiniLM model directory: {e}"))?;

    {
        let mut in_progress = in_progress_downloads()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !in_progress.insert(dest_dir.clone()) {
            return Err(
                "MiniLM model is already downloading — wait for it to finish before retrying"
                    .to_string(),
            );
        }
    }
    let _guard = DownloadGuard(dest_dir.clone());

    for file in FILES {
        let dest = dest_dir.join(file.dest_name);
        if file_is_complete(&dest, file.expected_size_bytes) {
            continue;
        }
        download_one(client, file, &dest).await?;
    }

    if !is_downloaded(models_root) {
        return Err("MiniLM download finished but the files are not the expected size".to_string());
    }
    Ok(dest_dir)
}

async fn download_one(
    client: &reqwest::Client,
    file: ModelFile,
    dest: &Path,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let partial = dest.with_extension(format!(
        "{}.partial",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    let response = client
        .get(file.url())
        .send()
        .await
        .map_err(|e| format!("failed to start download of {}: {e}", file.name))?
        .error_for_status()
        .map_err(|e| format!("download of {} failed: {e}", file.name))?;

    let mut out = tokio::fs::File::create(&partial)
        .await
        .map_err(|e| format!("failed to create {}: {e}", partial.display()))?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download of {} interrupted: {e}", file.name))?;
        out.write_all(&chunk)
            .await
            .map_err(|e| format!("failed writing {}: {e}", partial.display()))?;
    }
    out.flush()
        .await
        .map_err(|e| format!("failed flushing {}: {e}", partial.display()))?;
    drop(out);

    let actual_size = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
    if actual_size != file.expected_size_bytes {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "{} downloaded as {actual_size} bytes, expected {} — discarded",
            file.name, file.expected_size_bytes
        ));
    }

    std::fs::rename(&partial, dest)
        .map_err(|e| format!("failed to finalize {}: {e}", dest.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_dir_nests_under_the_named_folder() {
        let root = Path::new("/tmp/models");
        assert_eq!(model_dir(root), root.join("all-minilm-l6-v2-onnx"));
    }

    #[test]
    fn is_downloaded_is_false_when_the_directory_does_not_exist() {
        let root = std::env::temp_dir().join(format!("aside-minilm-test-{}", uuid::Uuid::new_v4()));
        assert!(!is_downloaded(&root));
    }

    #[test]
    fn is_downloaded_is_false_when_a_file_is_the_wrong_size() {
        let root = std::env::temp_dir().join(format!("aside-minilm-test-{}", uuid::Uuid::new_v4()));
        let dir = model_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        for f in FILES {
            std::fs::write(dir.join(f.dest_name), vec![0u8; 10]).unwrap(); // wrong size on purpose
        }
        assert!(!is_downloaded(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn is_downloaded_is_true_when_every_file_matches_its_expected_size() {
        let root = std::env::temp_dir().join(format!("aside-minilm-test-{}", uuid::Uuid::new_v4()));
        let dir = model_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        for f in FILES {
            std::fs::write(
                dir.join(f.dest_name),
                vec![0u8; f.expected_size_bytes as usize],
            )
            .unwrap();
        }
        assert!(is_downloaded(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn urls_point_at_the_stable_resolve_main_path() {
        for f in FILES {
            assert!(f
                .url()
                .starts_with("https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/"));
        }
    }
}
