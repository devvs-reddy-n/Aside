use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::active_session::{self, ActiveSessionInfo};
use crate::domain::{Session, SessionStatus, SessionType};
use crate::llm::LlmProviderKind;
use crate::storage::{copilots, documents, sessions, settings, Db};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotInput {
    pub name: String,
    pub company: Option<String>,
    pub target_role: Option<String>,
    pub job_description: Option<String>,
    pub instructions: Option<String>,
    pub save_transcript: bool,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    #[serde(default)]
    pub kind: copilots::CopilotKind,
    #[serde(default)]
    pub resume_id: Option<Uuid>,
    #[serde(default)]
    pub document_ids: Vec<Uuid>,
}

/// Saved as a Copilot's model when it should follow the default model in
/// Settings instead of a model of its own.
pub const DEFAULT_MODEL_CHOICE: &str = "default";

/// The provider and model a call from this Copilot uses: its own choice, or
/// the Settings default at the moment the call starts. The call snapshot
/// stores the resolved pair, so History always names the real model.
fn resolve_model(
    conn: &rusqlite::Connection,
    copilot: &copilots::Copilot,
) -> Result<(LlmProviderKind, String), String> {
    // The Settings switch decides the provider. A Copilot's own model counts
    // only while it belongs to that provider and is still offered; otherwise
    // the call follows the default.
    let selected = crate::llm::selected_provider(conn)?;
    if copilot.llm_model != DEFAULT_MODEL_CHOICE
        && copilot.llm_provider == selected
        && selected.supports_model(&copilot.llm_model)
    {
        return Ok((copilot.llm_provider, copilot.llm_model.clone()));
    }
    resolve_default_model(conn)
}

fn resolve_default_model(conn: &rusqlite::Connection) -> Result<(LlmProviderKind, String), String> {
    let provider = crate::llm::selected_provider(conn)?;
    Ok((provider, crate::llm::effective_model(conn, provider)?))
}

fn ensure_supported_kind(kind: copilots::CopilotKind) -> Result<(), String> {
    if kind == copilots::CopilotKind::Screen {
        return Err("This older call setup is no longer supported. Create a new Copilot.".into());
    }
    Ok(())
}

fn validate(input: &CopilotInput) -> Result<(), String> {
    ensure_supported_kind(input.kind)?;
    if input.name.trim().is_empty() {
        return Err("Give this Copilot a name first.".into());
    }
    if input.name.chars().count() > 100 {
        return Err("Copilot names must be 100 characters or fewer.".into());
    }
    if input.llm_model.trim().is_empty() {
        return Err("Choose an AI model for this Copilot.".into());
    }
    let model = input.llm_model.trim();
    if model != DEFAULT_MODEL_CHOICE && !input.llm_provider.supports_model(model) {
        return Err("Choose one of the listed AI models for this Copilot.".into());
    }
    Ok(())
}

fn build(id: Uuid, created_at_ms: u64, input: CopilotInput) -> copilots::Copilot {
    copilots::Copilot {
        id,
        name: input.name.trim().to_owned(),
        company: optional_text(input.company),
        target_role: optional_text(input.target_role),
        job_description: optional_text(input.job_description),
        instructions: optional_text(input.instructions),
        save_transcript: input.save_transcript,
        llm_provider: input.llm_provider,
        llm_model: input.llm_model.trim().to_owned(),
        kind: input.kind,
        created_at_ms,
        updated_at_ms: now_ms(),
    }
}

#[tauri::command]
pub fn list_copilots(db: State<Db>) -> Result<Vec<copilots::CopilotSummary>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    copilots::list(&conn)
}

#[tauri::command]
pub fn get_copilot_documents(
    db: State<Db>,
    copilot_id: Uuid,
) -> Result<Vec<documents::StoredDocument>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    copilots::document_ids(&conn, copilot_id)?
        .into_iter()
        .map(|id| {
            documents::get(&conn, id)?
                .ok_or_else(|| "A source in this Copilot was removed.".to_string())
        })
        .collect()
}

#[tauri::command]
pub fn create_copilot(db: State<Db>, input: CopilotInput) -> Result<copilots::Copilot, String> {
    validate(&input)?;
    let created = now_ms();
    let copilot = build(Uuid::new_v4(), created, input.clone());
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    copilots::create(&conn, &copilot)?;
    copilots::replace_documents(
        &conn,
        copilot.id,
        input.resume_id,
        &input.document_ids,
        now_ms(),
    )?;
    Ok(copilot)
}

#[tauri::command]
pub fn update_copilot(
    db: State<Db>,
    copilot_id: Uuid,
    input: CopilotInput,
) -> Result<copilots::Copilot, String> {
    validate(&input)?;
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let existing = copilots::get(&conn, copilot_id)?
        .ok_or_else(|| "That Copilot no longer exists.".to_string())?;
    let copilot = build(copilot_id, existing.created_at_ms, input.clone());
    if !copilots::update(&conn, &copilot)? {
        return Err("That Copilot no longer exists.".into());
    }
    copilots::replace_documents(
        &conn,
        copilot.id,
        input.resume_id,
        &input.document_ids,
        now_ms(),
    )?;
    Ok(copilot)
}

#[tauri::command]
pub fn delete_copilot(db: State<Db>, copilot_id: Uuid) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    copilots::delete(&conn, copilot_id)
}

#[derive(Debug, Clone, Serialize)]
pub struct StartedCopilotCall {
    pub session: Session,
}

/// Starts a new, active call from saved Copilot state. Its setup is copied
/// into the call record before capture begins, so later edits stay safely in
/// the template and can never rewrite an in-progress or historical call.
#[tauri::command]
pub fn start_copilot_call(
    app: AppHandle,
    db: State<Db>,
    copilot_id: Uuid,
) -> Result<StartedCopilotCall, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let copilot = copilots::get(&conn, copilot_id)?
        .ok_or_else(|| "That Copilot no longer exists.".to_string())?;
    ensure_supported_kind(copilot.kind)?;
    let (llm_provider, llm_model) = resolve_model(&conn, &copilot)?;
    let instructions = copilot.instructions.clone();
    if settings::get(&conn, llm_provider.api_key_setting())?.is_none() {
        return Err("This Copilot's AI provider needs an API key. Add it in Settings, then start the call again.".into());
    }
    let mut session = Session::new(
        copilot
            .target_role
            .clone()
            .unwrap_or_else(|| copilot.name.clone()),
        now_ms(),
    );
    session.company = copilot.company.clone();
    session.job_description = copilot.job_description.clone();
    session.save_transcript = copilot.save_transcript;
    session.session_type = SessionType::Interview;
    session.status = SessionStatus::Active;
    sessions::create(&conn, &session)?;
    let source_ids = copilots::document_ids(&conn, copilot_id)?;
    let source_names: Vec<String> = source_ids
        .iter()
        .filter_map(|id| {
            documents::get(&conn, *id)
                .ok()
                .flatten()
                .map(|d| d.file_name)
        })
        .collect();
    let snapshot = serde_json::json!({
        "copilot_name": copilot.name, "company": copilot.company, "target_role": copilot.target_role,
        "job_description": copilot.job_description, "instructions": instructions,
        "kind": copilot.kind,
        "save_transcript": copilot.save_transcript,
        "llm_provider": llm_provider, "llm_model": llm_model, "source_names": source_names,
    });
    sessions::set_copilot_snapshot(&conn, session.id, copilot_id, &snapshot.to_string())?;
    if let Some(instructions) = instructions.as_deref() {
        crate::storage::instructions::upsert(&conn, session.id, instructions, now_ms())?;
    }
    for document_id in source_ids {
        let doc = documents::get(&conn, document_id)?.ok_or_else(|| {
            "A source in this Copilot was removed. Open it and save again.".to_string()
        })?;
        documents::attach_to_session(&conn, session.id, document_id, &doc.doc_type, now_ms())?;
        let _ = crate::facts::pack::ingest_document_pack(
            &conn,
            session.id,
            &doc.raw_text,
            copilot.company.as_deref(),
            now_ms(),
        );
        let _ = crate::rag::indexer::index_document(&app, session.id, document_id, &doc.raw_text);
    }
    drop(conn);
    active_session::set(
        &app,
        ActiveSessionInfo {
            id: Some(session.id),
            session_type: session.session_type,
            company: session.company.clone(),
            job_description: session.job_description.clone(),
            instructions,
            save_transcript: session.save_transcript,
        },
    );
    crate::window_memory::warm_defaults(&app);
    Ok(StartedCopilotCall { session })
}

/// Persist the selected resume and today's global model as one unit. A failed
/// attachment cannot leave a half-created call in History.
fn create_resume_quick_call(
    conn: &rusqlite::Connection,
    resume_id: Uuid,
) -> Result<(Session, documents::StoredDocument), String> {
    let resume = documents::get(conn, resume_id)?.ok_or_else(|| {
        "That resume is no longer in your Library. Choose another resume.".to_string()
    })?;
    if resume.doc_type != "resume" {
        return Err("Choose a resume to start a quick call.".into());
    }
    if resume.raw_text.trim().is_empty() {
        return Err("This resume has no readable text. Replace it in Library first.".into());
    }
    let (provider, model) = resolve_default_model(conn)?;
    if settings::get(conn, provider.api_key_setting())?
        .filter(|key| !key.trim().is_empty())
        .is_none()
    {
        return Err("Add an API key for your default model in Settings first.".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut session = Session::new("Quick recruiter call", now_ms());
    session.session_type = SessionType::Interview;
    session.status = SessionStatus::Active;
    session.save_transcript = true;
    sessions::create(&tx, &session)?;
    let snapshot = serde_json::json!({
        "launch_source": "resume_quick_call", "copilot_name": "Quick recruiter call",
        "company": null, "target_role": null, "job_description": null,
        "instructions": null, "kind": "interview", "save_transcript": true,
        "llm_provider": provider, "llm_model": model, "source_names": [&resume.file_name],
    });
    sessions::set_call_snapshot(&tx, session.id, None, &snapshot.to_string())?;
    documents::attach_to_session(&tx, session.id, resume.id, "resume", now_ms())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok((session, resume))
}

/// Library shortcut: the chosen resume is enough. The default model and
/// global answer rules apply; no saved setup or job description is needed.
#[tauri::command]
pub fn start_resume_quick_call(
    app: AppHandle,
    db: State<Db>,
    resume_id: Uuid,
) -> Result<StartedCopilotCall, String> {
    if active_session::get(&app).is_some() {
        return Err("A call is already running. End it before starting another.".into());
    }
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let (session, resume) = create_resume_quick_call(&conn, resume_id)?;
    let _ = crate::facts::pack::ingest_document_pack(
        &conn,
        session.id,
        &resume.raw_text,
        None,
        now_ms(),
    );
    let _ = crate::rag::indexer::index_document(&app, session.id, resume.id, &resume.raw_text);
    drop(conn);
    active_session::set(
        &app,
        ActiveSessionInfo {
            id: Some(session.id),
            session_type: session.session_type,
            company: None,
            job_description: None,
            instructions: None,
            save_transcript: session.save_transcript,
        },
    );
    crate::window_memory::warm_defaults(&app);
    Ok(StartedCopilotCall { session })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_copilot_needs_a_name_and_model() {
        let input = CopilotInput {
            name: " ".into(),
            company: None,
            target_role: None,
            job_description: None,
            instructions: None,
            save_transcript: true,
            llm_provider: LlmProviderKind::OpenAi,
            llm_model: "".into(),
            kind: copilots::CopilotKind::Interview,
            resume_id: None,
            document_ids: vec![],
        };
        assert!(validate(&input).is_err());
    }

    fn copilot_with_model(model: &str) -> copilots::Copilot {
        copilots::Copilot {
            id: Uuid::new_v4(),
            name: "Nimbus".into(),
            company: None,
            target_role: None,
            job_description: None,
            instructions: None,
            save_transcript: true,
            llm_provider: LlmProviderKind::OpenAi,
            llm_model: model.into(),
            kind: copilots::CopilotKind::Interview,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn retired_setup_stays_readable_but_cannot_start() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut old = copilot_with_model(DEFAULT_MODEL_CHOICE);
        old.kind = copilots::CopilotKind::Screen;
        old.instructions = Some("Saved context".into());
        copilots::create(&conn, &old).unwrap();
        let restored = copilots::get(&conn, old.id).unwrap().unwrap();
        assert_eq!(restored.instructions.as_deref(), Some("Saved context"));
        assert!(ensure_supported_kind(restored.kind).is_err());
        assert!(ensure_supported_kind(copilot_with_model(DEFAULT_MODEL_CHOICE).kind).is_ok());
    }

    #[test]
    fn default_follows_settings_and_an_own_model_stays_put() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let first = resolve_model(&conn, &copilot_with_model(DEFAULT_MODEL_CHOICE)).unwrap();
        assert_eq!(first.0, LlmProviderKind::OpenAi);
        assert_eq!(first.1, LlmProviderKind::OpenAi.model_presets()[0]);
        settings::set(
            &conn,
            LlmProviderKind::OpenAi.model_setting(),
            "gpt-5.6-terra",
        )
        .unwrap();
        let chosen = resolve_model(&conn, &copilot_with_model(DEFAULT_MODEL_CHOICE)).unwrap();
        assert_eq!(chosen.1, "gpt-5.6-terra");
        let own = resolve_model(&conn, &copilot_with_model("gpt-6-luna")).unwrap();
        assert_eq!(own.1, "gpt-6-luna");
    }

    #[test]
    fn switching_to_claude_makes_every_copilot_use_claude() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        settings::set(&conn, crate::llm::PROVIDER_SETTING_KEY, "anthropic").unwrap();
        // A Copilot that picked a GPT model follows the Claude default.
        let gpt = resolve_model(&conn, &copilot_with_model("gpt-6.1-sol")).unwrap();
        assert_eq!(gpt, (LlmProviderKind::Anthropic, "claude-opus-5-5".to_string()));
        settings::set(&conn, LlmProviderKind::Anthropic.model_setting(), "claude-sonnet-5-5").unwrap();
        let default = resolve_model(&conn, &copilot_with_model(DEFAULT_MODEL_CHOICE)).unwrap();
        assert_eq!(default, (LlmProviderKind::Anthropic, "claude-sonnet-5-5".to_string()));
        let mut own = copilot_with_model("claude-haiku-5-5");
        own.llm_provider = LlmProviderKind::Anthropic;
        assert_eq!(resolve_model(&conn, &own).unwrap().1, "claude-haiku-5-5");
    }
    #[test]
    fn quick_call_uses_only_the_chosen_resume_and_freezes_the_global_model() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let resume =
            documents::insert_document(&conn, None, "resume", "Chosen.pdf", "Chosen experience", 1)
                .unwrap();
        documents::insert_document(
            &conn,
            None,
            "resume",
            "Other.pdf",
            "Unrelated experience",
            2,
        )
        .unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "test-key").unwrap();
        settings::set(
            &conn,
            LlmProviderKind::OpenAi.model_setting(),
            "gpt-5.6-terra",
        )
        .unwrap();
        let (call, _) = create_resume_quick_call(&conn, resume).unwrap();
        assert!(call.job_description.is_none());
        assert!(call.company.is_none());
        let attached = documents::list_attached_for_session(&conn, call.id).unwrap();
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].id, resume);
        assert!(copilots::list(&conn).unwrap().is_empty());
        settings::set(
            &conn,
            LlmProviderKind::OpenAi.model_setting(),
            "gpt-6-luna",
        )
        .unwrap();
        assert_eq!(
            sessions::copilot_model_for_call(&conn, call.id)
                .unwrap()
                .unwrap()
                .1,
            "gpt-5.6-terra"
        );
        let (next, _) = create_resume_quick_call(&conn, resume).unwrap();
        assert_eq!(
            sessions::copilot_model_for_call(&conn, next.id)
                .unwrap()
                .unwrap()
                .1,
            "gpt-6-luna"
        );
    }

    #[test]
    fn quick_call_rejects_missing_non_resume_empty_and_unconfigured_sources() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert!(create_resume_quick_call(&conn, Uuid::new_v4()).is_err());
        let note = documents::insert_document(&conn, None, "other", "Notes", "Notes", 1).unwrap();
        assert!(create_resume_quick_call(&conn, note).is_err());
        let empty = documents::insert_document(&conn, None, "resume", "Empty", " ", 1).unwrap();
        assert!(create_resume_quick_call(&conn, empty).is_err());
        let resume =
            documents::insert_document(&conn, None, "resume", "Resume", "Experience", 1).unwrap();
        assert!(create_resume_quick_call(&conn, resume).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn quick_call_rolls_back_if_its_resume_cannot_be_attached() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let resume =
            documents::insert_document(&conn, None, "resume", "Resume", "Experience", 1).unwrap();
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "test-key").unwrap();
        conn.execute_batch("CREATE TEMP TRIGGER reject_attachment BEFORE INSERT ON session_documents BEGIN SELECT RAISE(ABORT, 'test attachment failure'); END;").unwrap();
        assert!(create_resume_quick_call(&conn, resume).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
