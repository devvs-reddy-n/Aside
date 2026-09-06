use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::audio::channel::Channel;
use crate::pipeline::{self, TranscriptionRouting};
use crate::storage::{settings, Db};
use crate::stt::deepgram::DeepgramConfig;
use crate::stt::deepgram_live::DeepgramLiveEngine;
use crate::stt::parakeet_cache;
use crate::stt::parakeet_local::ParakeetLocalEngine;
use crate::stt::speechmatics::SpeechmaticsConfig;
use crate::stt::speechmatics_live::SpeechmaticsLiveEngine;
use crate::stt::{EngineKind, SttEngine};

const ENGINE_SETTING_KEY: &str = "stt_engine";

/// Engines currently offered for selection — `EngineKind::ENABLED`, not
/// `ALL` (Deepgram is dormant as of 2026-08-24, see `EngineKind::ENABLED`'s
/// doc comment). Deliberately not a hand-copied list here: it's the enum's
/// own `ENABLED` constant, so a variant added or re-enabled there can't
/// silently be left off this dropdown.
#[tauri::command]
pub fn list_stt_engines() -> Vec<EngineKind> {
    EngineKind::ENABLED.to_vec()
}

/// The currently selected engine, or `None` if the user hasn't picked one
/// yet (first-run onboarding, `status.md` Phase 11.1, hasn't landed).
#[tauri::command]
pub fn get_stt_engine(db: State<Db>) -> Result<Option<EngineKind>, String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let stored = settings::get(&conn, ENGINE_SETTING_KEY)?;
    Ok(stored.and_then(|s| EngineKind::from_setting_value(&s)))
}

/// Pure guard, split out from the `State`-taking command below so it's
/// directly unit-testable without standing up a real `tauri::State` (which
/// has no public test-friendly constructor). Rejects a dormant engine even
/// if something other than this app's own dropdown tries to set it — e.g.
/// a stale frontend build still offering Deepgram, or a hand-crafted
/// `invoke` call. The dropdown hiding it (`list_stt_engines`) and this
/// check are two independent layers on purpose: hiding it from a list is a
/// UI nicety, not an enforcement boundary.
fn reject_if_dormant(engine: EngineKind) -> Result<(), String> {
    if !engine.is_enabled() {
        return Err(format!(
            "{engine:?} is currently dormant and cannot be selected"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn set_stt_engine(db: State<Db>, engine: EngineKind) -> Result<(), String> {
    reject_if_dormant(engine)?;
    persist_stt_engine(&db, engine)
}

fn persist_stt_engine(db: &Db, engine: EngineKind) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(&conn, ENGINE_SETTING_KEY, engine.as_setting_value())
}

/// Stored plaintext, matching `docs/decisions.md`'s deliberate scope
/// decision (no keychain/encryption — this app is never published).
#[tauri::command]
pub fn set_stt_api_key(db: State<Db>, engine: EngineKind, api_key: String) -> Result<(), String> {
    let api_key = crate::stt::normalize_api_key(&api_key)?;
    let key_setting = engine
        .api_key_setting()
        .ok_or_else(|| format!("{engine:?} does not use an API key"))?;
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    settings::set(&conn, key_setting, &api_key)
}

/// Whether a key is saved for `engine` — deliberately never returns the key
/// itself to the frontend once saved, only presence, so a settings screen
/// can show "configured" without ever re-displaying the secret. Engines
/// that don't need a key (local Parakeet) report `true` — there is nothing
/// missing for them.
#[tauri::command]
pub fn has_stt_api_key(db: State<Db>, engine: EngineKind) -> Result<bool, String> {
    let Some(key_setting) = engine.api_key_setting() else {
        return Ok(true);
    };
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    Ok(settings::get(&conn, key_setting)?.is_some())
}

fn get_api_key(db: &Db, engine: EngineKind) -> Result<String, String> {
    let key_setting = engine
        .api_key_setting()
        .ok_or_else(|| format!("{engine:?} does not use an API key"))?;
    let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
    let raw = settings::get(&conn, key_setting)?
        .ok_or_else(|| format!("no API key saved for {engine:?} — set one first"))?;
    crate::stt::normalize_api_key(&raw)
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?
        .join("models"))
}

#[tauri::command]
pub fn is_parakeet_model_downloaded(app: AppHandle) -> Result<bool, String> {
    Ok(parakeet_cache::is_downloaded(&models_dir(&app)?))
}

/// Downloads the one Parakeet Realtime EOU 120M model if it is not already
/// present. Real network I/O — about 480 MB; the frontend should show this
/// as a loading state, not a fire-and-forget click.
#[tauri::command]
pub async fn download_parakeet_model(app: AppHandle) -> Result<(), String> {
    let dir = models_dir(&app)?;
    let client = reqwest::Client::new();
    parakeet_cache::ensure_downloaded(&client, &dir)
        .await
        .map(|_| ())
}

// --- Building a live engine from current settings ---

/// Constructs the currently-configured engine for `engine`, ready to hand to
/// `pipeline::start`. The one place that turns "which engine + which
/// settings" into a real, running `SttEngine` — so `start_channel_transcription`
/// below and any future caller (e.g. a one-shot "test this engine" command)
/// share the exact same construction logic rather than two copies that
/// could drift on which setting keys matter for which engine.
fn session_glossary(app: &AppHandle, db: &Db) -> Vec<String> {
    let active = crate::active_session::get(app);
    let mut terms = crate::stt::speechmatics::vocab_from_session(
        active.as_ref().and_then(|a| a.company.as_deref()),
        active.as_ref().and_then(|a| a.job_description.as_deref()),
    );
    if let Some(sid) = active.as_ref().and_then(|a| a.id) {
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(gl) =
            crate::storage::knowledge::glossary(&conn, sid, crate::storage::knowledge::GLOSSARY_CAP)
        {
            for t in gl {
                if !terms.iter().any(|e| e.eq_ignore_ascii_case(&t)) {
                    terms.push(t);
                }
                if terms.len() >= crate::storage::knowledge::GLOSSARY_CAP {
                    break;
                }
            }
        }
    }
    terms.truncate(crate::storage::knowledge::GLOSSARY_CAP);
    terms
}

async fn build_engine(
    app: &AppHandle,
    db: &Db,
    engine: EngineKind,
) -> Result<Box<dyn SttEngine>, String> {
    match engine {
        EngineKind::Deepgram => {
            let key = get_api_key(db, engine)?;
            let mut config = DeepgramConfig::default();
            config.keyterms = session_glossary(app, db);
            Ok(Box::new(DeepgramLiveEngine::start(key, config)))
        }
        EngineKind::Speechmatics => {
            let key = get_api_key(db, engine)?;
            let mut config = SpeechmaticsConfig::for_notebook();
            config.additional_vocab = session_glossary(app, db);
            // The overlay's More menu Language picker (2026-08-30) — used to
            // be local-only React state nothing ever read. Parakeet has no
            // runtime language switch (a fixed local model), so this is the
            // one engine where the picker actually changes anything.
            {
                let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
                config.language = crate::commands::overlay::current_language(&conn);
            }
            Ok(Box::new(SpeechmaticsLiveEngine::start(key, config)))
        }
        EngineKind::ParakeetLocal => {
            let dir = models_dir(app)?;
            if !parakeet_cache::is_downloaded(&dir) {
                return Err(
                    "Parakeet model is not downloaded yet — call download_parakeet_model first"
                        .to_string(),
                );
            }
            let path = parakeet_cache::model_dir(&dir);
            // Model load reads and parses ~480 MB of ONNX weights — blocking
            // work, kept off whatever async context called this command.
            // A second channel starting transcription reuses the already-loaded
            // weights (`parakeet_local::load`).
            tauri::async_runtime::spawn_blocking(move || ParakeetLocalEngine::load(&path))
                .await
                .map_err(|e| format!("model load task panicked: {e}"))?
                .map(|engine| Box::new(engine) as Box<dyn SttEngine>)
        }
    }
}

// --- Continuous transcription ---

/// Starts continuously transcribing `channel` with whatever engine is
/// currently selected in settings. Constructing the engine (an API key
/// lookup, or a local model load) happens here, on demand — not eagerly at
/// app startup, matching this project's "only the active engine's resources
/// ever exist" rule.
///
/// `session_id` is Phase 5.2's persistence plumbing (`pipeline::start`'s doc
/// comment has the full story): pass the id of a real `sessions` row (from
/// `create_session`) to have every final transcript this channel produces
/// written to `storage/transcripts.rs`, or `None` to run exactly as before —
/// live events only, nothing persisted. This command had no session concept
/// at all before this change; callers that want persistence now need to
/// create a session first and pass its id here.
#[tauri::command]
pub async fn start_channel_transcription(
    app: AppHandle,
    db: State<'_, Db>,
    routing: State<'_, TranscriptionRouting>,
    channel: Channel,
    session_id: Option<Uuid>,
) -> Result<(), String> {
    start_channel_transcription_inner(&app, db.inner(), routing.inner(), channel, session_id).await
}

/// The actual work of [`start_channel_transcription`], factored out so
/// `commands::capture::start_session_capture` can call it directly
/// (2026-08-25 — the session-start orchestrator that fixed the app starting
/// up deaf). Takes plain references rather than `State<'_, T>` extractors,
/// which only Tauri's own invoke machinery can construct — the same
/// command-wrapper/body split `commands::audio`'s `start_mic_capture_body`
/// already establishes.
pub(crate) async fn start_channel_transcription_inner(
    app: &AppHandle,
    db: &Db,
    routing: &TranscriptionRouting,
    channel: Channel,
    session_id: Option<Uuid>,
) -> Result<(), String> {
    let engine_kind = {
        let conn = db.0.lock().map_err(|_| "database poisoned".to_string())?;
        settings::get(&conn, ENGINE_SETTING_KEY)?
            .and_then(|s| EngineKind::from_setting_value(&s))
            .ok_or_else(|| "no STT engine selected yet — call set_stt_engine first".to_string())?
    };
    // Installed before the engine is built, not after — a local Parakeet
    // engine's first load this session is a real, measured ~400ms of ONNX
    // weight loading (see `stt::parakeet_local`'s
    // `measures_the_real_click_to_route_live_window` test), and any real
    // mic/system audio captured during that gap would otherwise have nowhere
    // to go and be silently lost rather than just delayed. See
    // `pipeline::prepare_route`'s doc comment for the full reasoning.
    let prepared = pipeline::prepare_route(routing, channel);
    let engine = match build_engine(app, db, engine_kind).await {
        Ok(engine) => engine,
        Err(err) => {
            pipeline::stop(routing, channel);
            return Err(err);
        }
    };
    pipeline::start_with_prepared_route(
        app.clone(),
        prepared,
        engine,
        session_id,
        engine_kind.origin_db_value(),
    );
    Ok(())
}

/// Stops `channel`'s transcription task. A no-op, not an error, if none is
/// running — matches `stop_mic_capture`'s own idempotent shape.
#[tauri::command]
pub fn stop_channel_transcription(
    routing: State<TranscriptionRouting>,
    channel: Channel,
) -> Result<(), String> {
    pipeline::stop(&routing, channel);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_api_key_reports_a_readable_error_for_an_engine_with_no_key_concept() {
        let db = Db::open_in_memory();
        let err = get_api_key(&db, EngineKind::ParakeetLocal).unwrap_err();
        assert!(err.contains("does not use an API key"));
    }

    #[test]
    fn get_api_key_reports_a_readable_error_when_nothing_is_saved_yet() {
        let db = Db::open_in_memory();
        let err = get_api_key(&db, EngineKind::Deepgram).unwrap_err();
        assert!(err.contains("no API key saved"));
    }

    #[test]
    fn get_api_key_returns_the_saved_key() {
        let db = Db::open_in_memory();
        {
            let conn = db.0.lock().unwrap();
            settings::set(
                &conn,
                EngineKind::Deepgram.api_key_setting().unwrap(),
                "dg_real",
            )
            .unwrap();
        }
        assert_eq!(get_api_key(&db, EngineKind::Deepgram).unwrap(), "dg_real");
    }

    #[test]
    fn list_stt_engines_omits_dormant_deepgram() {
        let listed = list_stt_engines();
        assert!(
            !listed.contains(&EngineKind::Deepgram),
            "Deepgram must not appear in the selectable list"
        );
        assert!(listed.contains(&EngineKind::ParakeetLocal));
        assert!(listed.contains(&EngineKind::Speechmatics));
    }

    #[test]
    fn reject_if_dormant_blocks_deepgram_but_not_the_enabled_engines() {
        let err = reject_if_dormant(EngineKind::Deepgram).unwrap_err();
        assert!(err.contains("dormant"));
        assert!(reject_if_dormant(EngineKind::ParakeetLocal).is_ok());
        assert!(reject_if_dormant(EngineKind::Speechmatics).is_ok());
    }

    #[test]
    fn persist_stt_engine_can_store_a_dormant_engine_directly() {
        // `persist_stt_engine` itself has no dormant/enabled opinion — that
        // check lives only in `reject_if_dormant`, called by `set_stt_engine`
        // before this helper. Confirms the helper stays a pure store, in
        // case any future caller needs to bypass the guard deliberately.
        let db = Db::open_in_memory();
        persist_stt_engine(&db, EngineKind::Deepgram).unwrap();
        let conn = db.0.lock().unwrap();
        let stored = settings::get(&conn, ENGINE_SETTING_KEY).unwrap();
        assert_eq!(stored.as_deref(), Some("deepgram"));
    }
}
