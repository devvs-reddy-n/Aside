//! Downloads and caches the one local Parakeet model Aside uses.
//!
//! One model, not a size picker: NVIDIA Parakeet Realtime EOU 120M v1
//! (English) in ONNX form (`altunenes/parakeet-rs` →
//! `realtime_eou_120m-v1-onnx`). File names and byte sizes match Hugging Face
//! `Content-Length` on the stable `resolve/main` URLs (2026-08-24). Never
//! hardcode a redirected CDN URL — those are signed and time-limited.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Folder name under `{app_data_dir}/models/` that holds this model's files.
pub const MODEL_DIR_NAME: &str = "parakeet-realtime-eou-120m-v1";

const HF_REPO: &str =
    "https://huggingface.co/altunenes/parakeet-rs/resolve/main/realtime_eou_120m-v1-onnx";

/// Every file `parakeet-rs`'s `ParakeetEOUHandle` loader needs in one directory.
pub const FILES: [ModelFile; 3] = [
    ModelFile {
        name: "encoder.onnx",
        expected_size_bytes: 459_341_289,
    },
    ModelFile {
        name: "decoder_joint.onnx",
        expected_size_bytes: 21_347_639,
    },
    ModelFile {
        name: "tokenizer.json",
        expected_size_bytes: 20_053,
    },
];

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub expected_size_bytes: u64,
}

impl ModelFile {
    pub fn url(self) -> String {
        format!("{HF_REPO}/{}", self.name)
    }
}

/// `{models_root}/parakeet-realtime-eou-120m-v1`.
pub fn model_dir(models_root: &Path) -> PathBuf {
    models_root.join(MODEL_DIR_NAME)
}

fn file_is_complete(path: &Path, expected: u64) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() == expected)
        .unwrap_or(false)
}

/// True only when every required file is present at its expected byte size.
/// A truncated download (crash mid-write) must not pass as "ready".
pub fn is_downloaded(models_root: &Path) -> bool {
    let dir = model_dir(models_root);
    FILES
        .iter()
        .all(|f| file_is_complete(&dir.join(f.name), f.expected_size_bytes))
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

/// Downloads any missing files into `{models_root}/parakeet-realtime-eou-120m-v1`.
/// Each file is written to a `.partial` sibling and renamed only when the
/// byte count matches, so a crash never leaves a truncated file at the
/// real path that `is_downloaded` would trust.
pub async fn ensure_downloaded(
    client: &reqwest::Client,
    models_root: &Path,
) -> Result<PathBuf, String> {
    let dest_dir = model_dir(models_root);
    if is_downloaded(models_root) {
        return Ok(dest_dir);
    }

    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("could not create Parakeet model directory: {e}"))?;

    {
        let mut in_progress = in_progress_downloads()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !in_progress.insert(dest_dir.clone()) {
            return Err(
                "Parakeet model is already downloading — wait for it to finish before retrying"
                    .to_string(),
            );
        }
    }
    let _guard = DownloadGuard(dest_dir.clone());

    for file in FILES {
        let dest = dest_dir.join(file.name);
        if file_is_complete(&dest, file.expected_size_bytes) {
            continue;
        }
        download_one(client, file, &dest).await?;
    }

    if !is_downloaded(models_root) {
        return Err(
            "Parakeet download finished but the files are not the expected size".to_string(),
        );
    }
    Ok(dest_dir)
}

async fn download_one(
    client: &reqwest::Client,
    file: ModelFile,
    dest: &Path,
) -> Result<(), String> {
    let partial = dest.with_file_name(format!("{}.partial", file.name));

    let resp = client
        .get(file.url())
        .send()
        .await
        .map_err(|e| format!("Parakeet download request failed for {}: {e}", file.name))?
        .error_for_status()
        .map_err(|e| format!("Parakeet download failed for {}: {e}", file.name))?;

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut out = tokio::fs::File::create(&partial)
        .await
        .map_err(|e| format!("could not create download file for {}: {e}", file.name))?;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download stream error for {}: {e}", file.name))?;
        written += chunk.len() as u64;
        out.write_all(&chunk)
            .await
            .map_err(|e| format!("could not write {}: {e}", file.name))?;
    }
    out.flush()
        .await
        .map_err(|e| format!("could not flush {}: {e}", file.name))?;
    drop(out);

    if written != file.expected_size_bytes {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(format!(
            "{} downloaded {written} bytes, expected {}",
            file.name, file.expected_size_bytes
        ));
    }

    tokio::fs::rename(&partial, dest)
        .await
        .map_err(|e| format!("could not finalize {}: {e}", file.name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_has_a_distinct_name_and_huggingface_url() {
        let mut names = std::collections::HashSet::new();
        for file in FILES {
            assert!(names.insert(file.name), "duplicate file name {}", file.name);
            assert!(file.url().starts_with(
                "https://huggingface.co/altunenes/parakeet-rs/resolve/main/realtime_eou_120m-v1-onnx/"
            ));
            assert!(file.url().ends_with(file.name));
            assert!(file.expected_size_bytes > 0);
        }
        assert_eq!(FILES.len(), 3);
    }

    #[test]
    fn the_locked_eou_byte_sizes_match_the_research_spec() {
        assert_eq!(FILES[0].name, "encoder.onnx");
        assert_eq!(FILES[0].expected_size_bytes, 459_341_289);
        assert_eq!(FILES[1].name, "decoder_joint.onnx");
        assert_eq!(FILES[1].expected_size_bytes, 21_347_639);
        assert_eq!(FILES[2].name, "tokenizer.json");
        assert_eq!(FILES[2].expected_size_bytes, 20_053);
        assert_eq!(MODEL_DIR_NAME, "parakeet-realtime-eou-120m-v1");
    }

    #[test]
    fn file_is_complete_rejects_a_missing_path() {
        assert!(!file_is_complete(Path::new("/no/such/parakeet-file"), 10));
    }

    #[test]
    fn file_is_complete_rejects_a_size_mismatch() {
        let dir =
            std::env::temp_dir().join(format!("aside-parakeet-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tokenizer.json");
        std::fs::write(&path, b"too-short").unwrap();
        assert!(!file_is_complete(&path, 20_053));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_is_complete_accepts_an_exact_byte_count() {
        let dir =
            std::env::temp_dir().join(format!("aside-parakeet-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tokenizer.json");
        std::fs::write(&path, vec![0u8; 64]).unwrap();
        assert!(file_is_complete(&path, 64));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_downloaded_is_false_when_the_directory_is_missing() {
        let dir =
            std::env::temp_dir().join(format!("aside-parakeet-missing-{}", uuid::Uuid::new_v4()));
        assert!(!is_downloaded(&dir));
    }

    #[test]
    fn is_downloaded_is_false_when_only_some_files_are_present() {
        let root =
            std::env::temp_dir().join(format!("aside-parakeet-partial-{}", uuid::Uuid::new_v4()));
        let dir = model_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tokenizer.json"), vec![0u8; 20_053]).unwrap();
        assert!(
            !is_downloaded(&root),
            "a folder with only tokenizer.json must not count as a complete model"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_second_call_while_one_is_already_downloading_is_rejected_not_racing() {
        let root =
            std::env::temp_dir().join(format!("aside-parakeet-race-{}", uuid::Uuid::new_v4()));
        let dest = model_dir(&root);
        in_progress_downloads().lock().unwrap().insert(dest.clone());

        let client = reqwest::Client::new();
        let err = ensure_downloaded(&client, &root)
            .await
            .expect_err("a concurrent download for the same model must be rejected");
        assert!(err.contains("already downloading"));

        in_progress_downloads().lock().unwrap().remove(&dest);
        let _ = std::fs::remove_dir_all(&root);
    }
}
