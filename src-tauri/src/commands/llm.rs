//! Tauri commands for Phase 6 — provider/key settings (mirrors
//! `commands/stt.rs`'s shape exactly) plus one manual test entry point.

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::llm::{self, LlmProviderKind};
use crate::storage::{settings, Db};

/// The providers offered: OpenAI (GPT-5.6) and Anthropic (Claude).
#[tauri::command]
pub fn list_llm_providers() -> Vec<LlmProviderKind> {
    LlmProviderKind::ALL.to_vec()
}

/// The currently selected *primary* provider, or `None` if the user hasn't
/// picked one yet.
#[tauri::command]
pub fn get_llm_provider(db: State<Db>) -> Result<Option<LlmProviderKind>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let stored = settings::get(&conn, llm::PROVIDER_SETTING_KEY)?;
    Ok(stored.and_then(|s| LlmProviderKind::from_setting_value(&s)))
}

#[tauri::command]
pub fn set_llm_provider(
    app: AppHandle,
    db: State<Db>,
    provider: LlmProviderKind,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(
        &conn,
        llm::PROVIDER_SETTING_KEY,
        provider.as_setting_value(),
    )?;
    drop(conn);
    llm::invalidate_llm_settings(&app);
    Ok(())
}

/// Stored plaintext, matching `docs/decisions.md`'s deliberate scope
/// decision (no keychain/encryption — this app is never published), same
/// as every STT API key.
#[tauri::command]
pub fn set_llm_api_key(
    app: AppHandle,
    db: State<Db>,
    provider: LlmProviderKind,
    api_key: String,
) -> Result<(), String> {
    crate::stt::validate_api_key(&api_key)?;
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(&conn, provider.api_key_setting(), &api_key)?;
    drop(conn);
    llm::invalidate_llm_settings(&app);
    Ok(())
}

// --- Model selection ---

/// The models offered for `provider`: a closed list of the latest
/// generation (see each provider's `MODEL_PRESETS`).
#[tauri::command]
pub fn list_llm_models(provider: LlmProviderKind) -> Vec<String> {
    provider
        .model_presets()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The model saved for `provider`, or `None` — in which case that provider
/// uses its own built-in default.
#[tauri::command]
pub fn get_llm_model(db: State<Db>, provider: LlmProviderKind) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::get(&conn, provider.model_setting())
}

/// Only the listed latest models are accepted, so an old or mistyped name
/// can't become an opaque provider-side 400 mid-interview.
fn validate_model(provider: LlmProviderKind, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if provider.supports_model(trimmed) {
        Ok(trimmed.to_string())
    } else {
        Err(format!(
            "Choose one of the listed models: {}.",
            provider.model_presets().join(", ")
        ))
    }
}

#[tauri::command]
pub fn set_llm_model(
    app: AppHandle,
    db: State<Db>,
    provider: LlmProviderKind,
    model: String,
) -> Result<(), String> {
    let validated = validate_model(provider, &model)?;
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(&conn, provider.model_setting(), &validated)?;
    drop(conn);
    llm::invalidate_llm_settings(&app);
    Ok(())
}

/// Paid OpenAI `flex` / `priority` tiers. Empty means off (the default).
#[tauri::command]
pub fn get_llm_service_tier(db: State<Db>) -> Result<String, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    Ok(settings::get(&conn, llm::SERVICE_TIER_SETTING_KEY)?.unwrap_or_default())
}

#[tauri::command]
pub fn set_llm_service_tier(app: AppHandle, db: State<Db>, tier: String) -> Result<(), String> {
    let normalized = crate::llm::openai::normalize_service_tier(Some(&tier));
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    if let Some(value) = normalized {
        settings::set(&conn, llm::SERVICE_TIER_SETTING_KEY, &value)?;
    } else {
        settings::delete(&conn, llm::SERVICE_TIER_SETTING_KEY)?;
    }
    drop(conn);
    llm::invalidate_llm_settings(&app);
    Ok(())
}

/// Whether a key is saved for `provider` — never returns the key itself,
/// same reasoning as `stt::has_stt_api_key`.
#[tauri::command]
pub fn has_llm_api_key(db: State<Db>, provider: LlmProviderKind) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    Ok(settings::get(&conn, provider.api_key_setting())?.is_some())
}

/// Manually asks for a suggested answer to `question`, streaming it through
/// the exact same `answer-delta`/`answer-done`/`answer-error` events (and
/// the exact same `llm::respond_to_question` path) the automatic
/// system-channel trigger uses. This is Phase 6's manual test entry point
/// — `SttBench.tsx` wires an "Ask AI" box to this, mirroring how the Bench
/// already exercises the STT engines directly, so the streaming path is
/// testable without needing a live conversation to happen to say something
/// that trips the auto-trigger. `session_id`, when given, exercises the
/// exact same RAG-retrieval path (`status.md` Phase 7) the automatic
/// trigger now uses — omit it (or pass `null`) to test the no-retrieval
/// path, matching `PanelWindow.tsx`'s existing ephemeral usage.
#[tauri::command]
pub async fn trigger_answer(
    app: AppHandle,
    question: String,
    session_id: Option<Uuid>,
    length: Option<crate::window_memory::LengthOverride>,
) -> Result<(), String> {
    if question.trim().is_empty() {
        return Err("question cannot be empty".to_string());
    }
    llm::respond_to_question(app, session_id, question, length).await;
    Ok(())
}

/// The overlay's Screenshot pill (spec §6.1 row 4) — a button a mouse can
/// click, doing exactly what `shortcuts.rs`'s ⌘⇧↩ global-shortcut path
/// (`capture_and_analyze`) already does, since a global shortcut's Rust
/// handler isn't itself something a frontend `onClick` can invoke directly.
/// Both paths end at the same `llm::respond_to_screenshot`, so a screenshot
/// taken from the button and one taken from the keyboard render identically.
#[tauri::command]
pub async fn trigger_screenshot_answer(app: AppHandle) -> Result<(), String> {
    let Some(claim) = llm::try_begin_user_answer(&app) else {
        return Err("Please wait a moment before trying again.".to_string());
    };
    let session_id = crate::active_session::get(&app).and_then(|s| s.id);
    let encoded = crate::screen::cache::fresh_jpeg_for_answer(&app).await;

    // `claim` is only ever consumed on the real-answer path below — every
    // other branch here returns without it, which drops it (and, via
    // `AnswerClaim`'s own `Drop`, clears the in-flight flag) exactly as
    // intended for a screenshot that never made it to a model.
    match encoded {
        Ok(jpeg_base64) => {
            llm::respond_to_screenshot(app, session_id, jpeg_base64, claim).await;
            Ok(())
        }
        Err(message) => Err(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_llm_provider_is_none_before_anything_is_set() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert_eq!(
            settings::get(&conn, llm::PROVIDER_SETTING_KEY).unwrap(),
            None
        );
    }

    #[test]
    fn set_then_get_provider_round_trips_through_the_command_layer() {
        let db = Db::open_in_memory();
        {
            let conn = db.0.lock().unwrap();
            settings::set(
                &conn,
                llm::PROVIDER_SETTING_KEY,
                LlmProviderKind::Anthropic.as_setting_value(),
            )
            .unwrap();
        }
        let conn = db.0.lock().unwrap();
        let stored = settings::get(&conn, llm::PROVIDER_SETTING_KEY).unwrap();
        assert_eq!(
            stored.and_then(|s| LlmProviderKind::from_setting_value(&s)),
            Some(LlmProviderKind::Anthropic)
        );
    }

    #[test]
    fn has_llm_api_key_is_false_until_one_is_saved() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert_eq!(
            settings::get(&conn, LlmProviderKind::OpenAi.api_key_setting()).unwrap(),
            None
        );
        settings::set(&conn, LlmProviderKind::OpenAi.api_key_setting(), "sk-real").unwrap();
        assert!(
            settings::get(&conn, LlmProviderKind::OpenAi.api_key_setting())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn list_llm_providers_offers_openai_and_anthropic() {
        assert_eq!(list_llm_providers(), vec![LlmProviderKind::OpenAi, LlmProviderKind::Anthropic]);
    }

    #[test]
    fn only_listed_latest_models_are_accepted() {
        for provider in LlmProviderKind::ALL {
            for model in provider.model_presets() {
                assert_eq!(validate_model(provider, &format!("  {model} ")).unwrap(), *model);
            }
        }
        for (provider, old) in [
            (LlmProviderKind::OpenAi, "gpt-5.5"),
            (LlmProviderKind::OpenAi, "gpt-5.4-mini"),
            (LlmProviderKind::OpenAi, "claude-opus-5-5"),
            (LlmProviderKind::Anthropic, "claude-sonnet-4-6"),
            (LlmProviderKind::Anthropic, ""),
        ] {
            let err = validate_model(provider, old).unwrap_err();
            assert!(err.contains("listed models"), "{provider:?} {old}: {err}");
        }
    }

    #[test]
    fn every_provider_has_a_distinct_model_setting_key() {
        let mut seen = std::collections::HashSet::new();
        for kind in LlmProviderKind::ALL {
            assert!(
                seen.insert(kind.model_setting()),
                "duplicate model_setting for {kind:?}"
            );
            // And it must not collide with the api-key key for the same provider.
            assert_ne!(kind.model_setting(), kind.api_key_setting());
        }
    }

    #[test]
    fn set_then_get_model_round_trips_through_the_settings_table() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert_eq!(
            settings::get(&conn, LlmProviderKind::OpenAi.model_setting()).unwrap(),
            None
        );
        settings::set(&conn, LlmProviderKind::OpenAi.model_setting(), "gpt-5.6-sol").unwrap();
        assert_eq!(
            settings::get(&conn, LlmProviderKind::OpenAi.model_setting()).unwrap(),
            Some("gpt-5.6-sol".to_string())
        );
    }

    #[test]
    fn list_llm_models_returns_a_non_empty_preset_list_for_every_provider() {
        for kind in LlmProviderKind::ALL {
            assert!(!list_llm_models(kind).is_empty(), "{kind:?} has no presets");
        }
    }
}

/// A display-only overview. The original setup text remains the call context.
#[tauri::command]
pub async fn summarize_setup_text(
    app: AppHandle,
    copilot_id: Option<String>,
    kind: String,
    text: String,
) -> Result<Vec<String>, String> {
    llm::setup_summary::summarize(&app, copilot_id.as_deref(), &kind, &text).await
}
