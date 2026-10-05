//! Whether the default AI model and speech engine can actually be reached,
//! for the two status dots at the bottom of the main window's sidebar.
//!
//! Each check makes one small real request with the saved key (OpenAI: look
//! up the chosen model; Speechmatics: mint a realtime token). Nothing is sent
//! for transcription or answering, and keys never leave the request header.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::llm::{HttpClient, LlmProviderKind, PROVIDER_SETTING_KEY};
use crate::storage::{settings, Db};
use crate::stt::speechmatics::{looks_like_jwt, SpeechmaticsConfig};
use crate::stt::speechmatics_live::{verify_key, KeyCheck};
use crate::stt::{parakeet_cache, EngineKind};

const OPENAI_MODELS_URL: &str = "https://api.openai.com/v1/models/";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Connected,
    Failed,
    /// No real check exists for this provider or engine yet.
    Unchecked,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Link {
    pub state: LinkState,
    /// One plain sentence for the status popover.
    pub detail: String,
}

impl Link {
    fn new(state: LinkState, detail: impl Into<String>) -> Self {
        Self { state, detail: detail.into() }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelConnections {
    pub ai: Link,
    pub speech: Link,
}

/// What OpenAI's answer to "look up this model" means for the dot.
fn openai_link(status: u16, model: &str) -> Link {
    match status {
        200..=299 => Link::new(LinkState::Connected, "Connected to OpenAI."),
        401 | 403 => Link::new(LinkState::Failed, "OpenAI rejected the saved API key."),
        404 => Link::new(LinkState::Failed, format!("OpenAI doesn't offer {model} to this key.")),
        429 => Link::new(LinkState::Failed, "OpenAI is limiting requests for this key right now."),
        _ => Link::new(LinkState::Failed, format!("OpenAI answered with an error ({status}).")),
    }
}

async fn check_ai(client: reqwest::Client, plan: Option<(LlmProviderKind, Option<String>, String)>) -> Link {
    let Some((provider, key, model)) = plan else {
        return Link::new(LinkState::Failed, "No AI model chosen yet.");
    };
    if provider != LlmProviderKind::OpenAi {
        return Link::new(LinkState::Unchecked, "Checked when a call starts.");
    }
    let Some(key) = key else {
        return Link::new(LinkState::Failed, "No API key saved.");
    };
    let mut url = match reqwest::Url::parse(OPENAI_MODELS_URL) {
        Ok(url) => url,
        Err(_) => return Link::new(LinkState::Failed, "Couldn't build the OpenAI address."),
    };
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.pop_if_empty().push(&model);
    }
    match client.get(url).bearer_auth(key).timeout(TIMEOUT).send().await {
        Ok(response) => openai_link(response.status().as_u16(), &model),
        Err(_) => Link::new(LinkState::Failed, "Couldn't reach OpenAI. Check the internet connection."),
    }
}

enum SpeechPlan {
    NoEngine,
    Parakeet { downloaded: bool },
    Speechmatics { key: Option<String> },
    Other,
}

async fn check_speech(plan: SpeechPlan) -> Link {
    match plan {
        SpeechPlan::NoEngine => Link::new(LinkState::Failed, "No speech engine chosen yet."),
        SpeechPlan::Parakeet { downloaded: true } => Link::new(LinkState::Connected, "Runs on this Mac."),
        SpeechPlan::Parakeet { downloaded: false } => Link::new(LinkState::Failed, "Parakeet isn't downloaded yet."),
        SpeechPlan::Speechmatics { key: None } => Link::new(LinkState::Failed, "No API key saved."),
        SpeechPlan::Speechmatics { key: Some(key) } if looks_like_jwt(&key) => {
            Link::new(LinkState::Unchecked, "Saved as a temporary token; checked when a call starts.")
        }
        SpeechPlan::Speechmatics { key: Some(key) } => {
            match verify_key(&key, SpeechmaticsConfig::for_notebook().region).await {
                KeyCheck::Valid => Link::new(LinkState::Connected, "Connected to Speechmatics."),
                KeyCheck::Rejected => Link::new(LinkState::Failed, "Speechmatics rejected the saved API key."),
                KeyCheck::Unreachable => Link::new(LinkState::Failed, "Couldn't reach Speechmatics. Check the internet connection."),
            }
        }
        SpeechPlan::Other => Link::new(LinkState::Unchecked, "Checked when a call starts."),
    }
}

/// Reaches the default AI model and the chosen speech engine, both at once.
#[tauri::command]
pub async fn check_model_connections(
    app: AppHandle,
    db: State<'_, Db>,
    http: State<'_, HttpClient>,
) -> Result<ModelConnections, String> {
    let models = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?
        .join("models");
    // Read everything first so the database is never locked during a request.
    let (ai_plan, speech_plan) = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        let provider = settings::get(&conn, PROVIDER_SETTING_KEY)?
            .and_then(|value| LlmProviderKind::from_setting_value(&value));
        let ai_plan = match provider {
            Some(provider) => {
                let key = settings::get(&conn, provider.api_key_setting())?
                    .map(|key| key.trim().to_owned())
                    .filter(|key| !key.is_empty());
                let model = settings::get(&conn, provider.model_setting())?
                    .filter(|model| !model.trim().is_empty())
                    .or_else(|| provider.model_presets().first().map(|model| model.to_string()))
                    .unwrap_or_default();
                Some((provider, key, model))
            }
            None => None,
        };
        let engine = settings::get(&conn, crate::commands::stt::ENGINE_SETTING_KEY)?
            .and_then(|value| EngineKind::from_setting_value(&value));
        let speech_plan = match engine {
            None => SpeechPlan::NoEngine,
            Some(EngineKind::ParakeetLocal) => SpeechPlan::Parakeet { downloaded: parakeet_cache::is_downloaded(&models) },
            Some(EngineKind::Speechmatics) => {
                let key = match EngineKind::Speechmatics.api_key_setting() {
                    Some(setting) => settings::get(&conn, setting)?,
                    None => None,
                };
                SpeechPlan::Speechmatics { key: key.and_then(|raw| crate::stt::normalize_api_key(&raw).ok()) }
            }
            Some(_) => SpeechPlan::Other,
        };
        (ai_plan, speech_plan)
    };
    let (ai, speech) = tokio::join!(check_ai(http.0.clone(), ai_plan), check_speech(speech_plan));
    Ok(ModelConnections { ai, speech })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_answers_map_to_plain_states() {
        assert_eq!(openai_link(200, "m").state, LinkState::Connected);
        assert_eq!(openai_link(401, "m").state, LinkState::Failed);
        assert!(openai_link(404, "gpt-x").detail.contains("gpt-x"));
        assert_eq!(openai_link(500, "m").state, LinkState::Failed);
    }

    #[test]
    fn missing_setup_fails_without_any_request() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let client = reqwest::Client::new();
        assert_eq!(rt.block_on(check_ai(client.clone(), None)).state, LinkState::Failed);
        let no_key = Some((LlmProviderKind::OpenAi, None, "gpt-5.6-luna".to_string()));
        assert_eq!(rt.block_on(check_ai(client, no_key)).detail, "No API key saved.");
        assert_eq!(rt.block_on(check_speech(SpeechPlan::Parakeet { downloaded: true })).state, LinkState::Connected);
        assert_eq!(rt.block_on(check_speech(SpeechPlan::Speechmatics { key: None })).state, LinkState::Failed);
    }
}
