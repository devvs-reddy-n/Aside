use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::active_session::{self, ActiveSessionInfo};
use crate::domain::{Session, SessionStatus, SessionType};
use crate::llm::{LlmProviderKind, PROVIDER_SETTING_KEY};
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
    if copilot.llm_model != DEFAULT_MODEL_CHOICE {
        return Ok((copilot.llm_provider, copilot.llm_model.clone()));
    }
    let provider = settings::get(conn, PROVIDER_SETTING_KEY)?
        .and_then(|value| LlmProviderKind::from_setting_value(&value))
        .unwrap_or(LlmProviderKind::OpenAi);
    let model = settings::get(conn, provider.model_setting())?
        .filter(|model| !model.trim().is_empty())
        .or_else(|| provider.model_presets().first().map(|model| model.to_string()))
        .ok_or_else(|| "Choose a default AI model in Settings first.".to_string())?;
    Ok((provider, model))
}

fn validate(input: &CopilotInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("Give this Copilot a name first.".into());
    }
    if input.name.chars().count() > 100 {
        return Err("Copilot names must be 100 characters or fewer.".into());
    }
    if input.llm_model.trim().is_empty() {
        return Err("Choose an AI model for this Copilot.".into());
    }
    if !input.llm_provider.is_enabled() {
        return Err("That AI provider is not available in this build. Choose a configured provider in Settings.".into());
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
    let (llm_provider, llm_model) = resolve_model(&conn, &copilot)?;
    if !llm_provider.is_enabled() {
        return Err("This Copilot's AI provider is unavailable. Choose another model in its setup or configure it in Settings.".into());
    }
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
        "job_description": copilot.job_description, "instructions": copilot.instructions,
        "save_transcript": copilot.save_transcript,
        "llm_provider": llm_provider, "llm_model": llm_model, "source_names": source_names,
    });
    sessions::set_copilot_snapshot(&conn, session.id, copilot_id, &snapshot.to_string())?;
    if let Some(instructions) = copilot.instructions.as_deref() {
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
            instructions: copilot.instructions,
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
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn default_follows_settings_and_an_own_model_stays_put() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let first = resolve_model(&conn, &copilot_with_model(DEFAULT_MODEL_CHOICE)).unwrap();
        assert_eq!(first.0, LlmProviderKind::OpenAi);
        assert_eq!(first.1, LlmProviderKind::OpenAi.model_presets()[0]);
        settings::set(&conn, LlmProviderKind::OpenAi.model_setting(), "gpt-5.6-terra").unwrap();
        let chosen = resolve_model(&conn, &copilot_with_model(DEFAULT_MODEL_CHOICE)).unwrap();
        assert_eq!(chosen.1, "gpt-5.6-terra");
        let own = resolve_model(&conn, &copilot_with_model("gpt-5.6-luna")).unwrap();
        assert_eq!(own.1, "gpt-5.6-luna");
    }
}
