//! Tauri commands for Phase 8 — resume/job-description ingestion
//! (`status.md` 8.4/8.5). Mirrors `commands/rag.rs`/`commands/llm.rs`'s
//! established shape: thin command functions that delegate to real logic
//! modules (`ingest::extract_text`, `storage::documents`,
//! `rag::indexer::index_document`) rather than doing extraction/storage
//! work themselves.
//!
//! **File picking happens on the frontend**, via `@tauri-apps/plugin-dialog`'s
//! `open()` — this command receives an already-chosen absolute path. Same
//! split this project already uses for API keys (the frontend collects the
//! string, Rust validates and persists it) rather than a Rust-side dialog
//! command; keeps the picker UI (title, filters, multi-select) entirely a
//! frontend concern.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::ingest::{self, DocType};
use crate::storage::{documents, Db};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Every doc type — for a picker dropdown, same reasoning as
/// `stt::list_stt_engines`/`llm::list_llm_providers`: this is
/// `DocType::ALL` itself, not a hand-copied list that could drift from it.
#[tauri::command]
pub fn list_doc_types() -> Vec<DocType> {
    DocType::ALL.to_vec()
}

/// Real return shape for the frontend: enough to show "uploaded X, got Y
/// characters of text, N chunks queued for indexing" without shipping the
/// full extracted text back over IPC a second time — it was already sent
/// once, as the file itself, and is now durably stored.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IngestedDocument {
    pub document_id: Uuid,
    pub file_name: String,
    pub doc_type: DocType,
    pub char_count: usize,
    /// How many chunks `rag::indexer::index_document` queued for
    /// background embedding — **queued, not yet necessarily written**;
    /// embedding + storage happens on a background task per chunk (same
    /// asynchronous shape live transcript indexing already uses), so a
    /// chunk could still be embedding when this command returns.
    pub chunks_queued: usize,
}

/// A private original-file read for the in-app document inspector.  The
/// app never returns the private on-disk path; the webview only receives
/// bytes and a MIME type with which it can make a temporary Blob URL.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentOriginal {
    pub mime_type: String,
    pub bytes_base64: String,
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        _ => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    }
}

fn originals_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not find Aside's private data folder: {e}"))?
        .join("documents");
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("could not create Aside's document library: {e}"))?;
    Ok(path)
}

/// Extracts text from `path` (already chosen by the frontend's file-picker
/// dialog), stores it as a document, and — when `session_id` is given —
/// feeds the extracted text into Phase 7's chunk/RAG pipeline
/// (`rag::indexer::index_document`), the real integration point `status.md`
/// 8.5 asks for. `session_id` is `Option<Uuid>` (2026-08-25 setup-screen
/// work, was a required `Uuid` before): the setup screen can upload a
/// resume *before* a session exists yet (no id to attach to at upload
/// time). With `session_id: None`, the document is stored but indexing is
/// skipped entirely — chunks are session-scoped and can't exist without one
/// — and `chunks_queued` is honestly `0`, not a lie. Call
/// `attach_document_to_session` afterward once a real session exists to
/// index it retroactively.
#[tauri::command]
pub async fn ingest_document(
    app: AppHandle,
    db: State<'_, Db>,
    session_id: Option<Uuid>,
    path: String,
    doc_type: DocType,
) -> Result<IngestedDocument, String> {
    let path_buf = std::path::PathBuf::from(&path);
    let file_name = path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "document".to_string());

    // The real parse (`pdf-extract`/`docx-rs`, both synchronous and
    // CPU-bound, with `ingest::extract_text`'s own size-scaled timeout
    // enforced inside) runs on a blocking thread so it never stalls the
    // async runtime's other work — same reasoning `rag::indexer`'s ONNX
    // embedder load already uses `spawn_blocking` for.
    let extract_path = path_buf.clone();
    let extracted =
        tauri::async_runtime::spawn_blocking(move || ingest::extract_text(&extract_path))
            .await
            .map_err(|e| format!("document extraction task panicked: {e}"))?;
    let text = extracted?;

    let document_id = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::insert_document(
            &conn,
            session_id,
            doc_type.db_value(),
            &file_name,
            &text,
            now_ms(),
        )?
    };

    // Copy only after extraction has succeeded. This prevents Aside from
    // keeping a file it could not read, and makes its private copy the one
    // the viewer uses even if the source file is later moved or deleted.
    let extension = path_buf
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let rel_path = format!(
        "documents/{}.{}",
        document_id,
        extension.to_ascii_lowercase()
    );
    let destination = originals_dir(&app)?.join(format!(
        "{}.{}",
        document_id,
        extension.to_ascii_lowercase()
    ));
    let byte_size = std::fs::metadata(&path_buf)
        .map_err(|e| format!("could not inspect selected file: {e}"))?
        .len();
    if let Err(error) = std::fs::copy(&path_buf, &destination) {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        let _ = documents::delete(&conn, document_id);
        return Err(format!(
            "could not keep a private copy of this file: {error}"
        ));
    }
    {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        if let Err(error) = documents::set_original_metadata(
            &conn,
            document_id,
            &rel_path,
            mime_for(&path_buf),
            byte_size,
        ) {
            let _ = std::fs::remove_file(&destination);
            let _ = documents::delete(&conn, document_id);
            return Err(error);
        }
    }

    finish_ingest(
        &app,
        &db,
        session_id,
        document_id,
        file_name,
        doc_type,
        text,
    )
}

/// The shared tail `ingest_document` and `ingest_text_document` both need
/// once a document row already exists: queue it for RAG indexing, feed it
/// into the facts pack, and warm the Documents & Context page's cache —
/// factored out so that tail exists in exactly one place, per the layer's
/// own instructions, rather than being copy-pasted between the file-upload
/// and pasted-text paths.
fn finish_ingest(
    app: &AppHandle,
    db: &State<Db>,
    session_id: Option<Uuid>,
    document_id: Uuid,
    file_name: String,
    doc_type: DocType,
    text: String,
) -> Result<IngestedDocument, String> {
    let chunks_queued = match session_id {
        Some(sid) => crate::rag::indexer::index_document(app, sid, document_id, &text),
        None => 0,
    };

    if let Some(sid) = session_id {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        // A document uploaded straight into a live session (unlike the
        // Create screen's upload-before-a-session-exists path, which relies
        // on `attach_document_to_session` for this instead) is unambiguously
        // "the one" for its `doc_type` here — record it the same way, so
        // `window_memory::load_default_context_from_store` doesn't fall back
        // to the oldest-row-wins library scan for this session either.
        documents::attach_to_session(&conn, sid, document_id, doc_type.db_value(), now_ms())?;
        let company = crate::active_session::get(app).and_then(|a| a.company);
        let _ = crate::facts::pack::ingest_document_pack(
            &conn,
            sid,
            &text,
            company.as_deref(),
            now_ms(),
        );
    }

    crate::window_memory::warm_defaults(app);
    Ok(IngestedDocument {
        document_id,
        file_name,
        doc_type,
        char_count: text.chars().count(),
        chunks_queued,
    })
}

/// Ingests raw pasted text — not a file — as a document. Gap 1: the setup
/// screen's stated requirement to "paste job description and extra context
/// directly, no file needed" (`CURRENT.md`), which `ingest_document` alone
/// can't cover since it only accepts a real file path. `name` is a
/// caller-supplied stable marker for the pasted field (e.g. "Pasted job
/// description") — `storage::documents::upsert_document` uses it, together
/// with `session_id`/`doc_type`, to replace a previous paste of the same
/// field instead of piling up a new document every time the owner edits
/// their draft. Synchronous, unlike `ingest_document`: there is no file to
/// parse on a blocking thread, `text` already is the extracted text.
#[tauri::command]
pub fn ingest_text_document(
    app: AppHandle,
    db: State<'_, Db>,
    session_id: Option<Uuid>,
    doc_type: DocType,
    name: String,
    text: String,
) -> Result<IngestedDocument, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("There is no text to save yet.".to_string());
    }

    let document_id = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::upsert_document(
            &conn,
            session_id,
            doc_type.db_value(),
            &name,
            trimmed,
            now_ms(),
        )?
    };

    // The active session's own `job_description` (set at `create_session`
    // time, e.g. from a saved draft) otherwise keeps winning over this
    // fresh document text — see `active_session::patch_job_description`'s
    // doc comment for why.
    if matches!(doc_type, DocType::JobDescription) {
        if let Some(sid) = session_id {
            crate::active_session::patch_job_description(
                &app,
                Some(sid),
                Some(trimmed.to_string()),
            );
        }
    }

    finish_ingest(
        &app,
        &db,
        session_id,
        document_id,
        name,
        doc_type,
        trimmed.to_string(),
    )
}

/// Permanently removes a document — Gap 2: the page could add documents but
/// never take one back, so a wrong upload or a stale paste was permanent.
/// One confirm step on the frontend (`Dialog`'s destructive mode), no trash
/// bin, matching this project's stated preference elsewhere. The schema's
/// `ON DELETE CASCADE` on `chunks.document_id` (`storage/migrations.rs`)
/// removes the document's search chunks in the same statement, so this
/// command doesn't need to touch `chunks` itself.
#[tauri::command]
pub fn delete_document(app: AppHandle, db: State<Db>, document_id: Uuid) -> Result<bool, String> {
    let original_rel_path = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::original_relative_path(&conn, document_id)?
    };
    let deleted = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::delete(&conn, document_id)?
    };
    if deleted {
        if let Some(relative) = original_rel_path {
            if let Ok(base) = app.path().app_data_dir() {
                let _ = std::fs::remove_file(base.join(relative));
            }
        }
    }
    crate::window_memory::warm_defaults(&app);
    Ok(deleted)
}

/// Reads Aside's retained original file when one exists. Legacy uploads have
/// no retained bytes by design; their parsed text remains available instead.
#[tauri::command]
pub fn read_document_original(
    app: AppHandle,
    db: State<Db>,
    document_id: Uuid,
) -> Result<DocumentOriginal, String> {
    let relative = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::original_relative_path(&conn, document_id)?.ok_or_else(|| {
            "This older upload has parsed text only. Re-upload it to view the original file."
                .to_string()
        })?
    };
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not find Aside's private data folder: {e}"))?;
    let path = base.join(relative);
    let bytes = std::fs::read(&path).map_err(|_| {
        "Aside could not read the private original file. Re-upload it to restore the preview."
            .to_string()
    })?;
    Ok(DocumentOriginal {
        mime_type: mime_for(&path).to_string(),
        bytes_base64: BASE64.encode(bytes),
    })
}

/// The `limit` most recently uploaded documents of `doc_type` — the setup
/// screen's "pick a previously uploaded resume" dropdown (2026-08-25).
#[tauri::command]
pub fn list_recent_documents(
    db: State<Db>,
    doc_type: DocType,
    limit: i64,
) -> Result<Vec<documents::StoredDocument>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    documents::list_recent(&conn, doc_type.db_value(), limit)
}

/// Re-indexes an already-uploaded document's extracted text under `session_id`
/// — the real path for the setup screen's "pick an existing resume" flow
/// (2026-08-25), where parsing already happened once and only needs to
/// happen again for the *chunking/embedding* half, since chunks are
/// session-scoped. Deliberately does **not** reassign the document row's
/// own `session_id` column — a document can be reused across many
/// sessions, so it stays attributed to whichever session first uploaded it;
/// only the chunks this call creates belong to the new session.
#[tauri::command]
pub async fn attach_document_to_session(
    app: AppHandle,
    db: State<'_, Db>,
    session_id: Uuid,
    document_id: Uuid,
) -> Result<usize, String> {
    let doc = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        documents::get(&conn, document_id)?.ok_or_else(|| "document not found".to_string())?
    };
    let queued = crate::rag::indexer::index_document(&app, session_id, document_id, &doc.raw_text);
    {
        let company = crate::active_session::get(&app).and_then(|a| a.company);
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        // The real fix for the "wrong resume answered" bug: this is exactly
        // the moment the owner's pick (Create screen's Resume/Documents
        // modal) is known, so record it — `load_default_context_from_store`
        // then prefers this over `list_for_context`'s oldest-row-wins scan
        // of the session-less document library.
        documents::attach_to_session(&conn, session_id, document_id, &doc.doc_type, now_ms())?;
        let _ = crate::facts::pack::ingest_document_pack(
            &conn,
            session_id,
            &doc.raw_text,
            company.as_deref(),
            now_ms(),
        );
    }
    crate::window_memory::warm_defaults(&app);
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_doc_types_reports_every_schema_accepted_value() {
        let kinds = list_doc_types();
        assert_eq!(kinds.len(), 3);
        assert!(kinds.contains(&DocType::Resume));
        assert!(kinds.contains(&DocType::JobDescription));
        assert!(kinds.contains(&DocType::Other));
    }

    #[test]
    fn doc_type_db_values_match_the_schemas_check_constraint_exactly() {
        // Pinned to `storage/migrations.rs`'s
        // `doc_type CHECK (doc_type IN ('resume','job_description','other'))`
        // — a drift here would make `insert_document` fail with a real
        // database error on every ingest, not silently misbehave, but this
        // catches it at test time instead of at first real upload.
        assert_eq!(DocType::Resume.db_value(), "resume");
        assert_eq!(DocType::JobDescription.db_value(), "job_description");
        assert_eq!(DocType::Other.db_value(), "other");
    }
}
