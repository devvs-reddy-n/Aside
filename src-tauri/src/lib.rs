mod active_session;
mod audio;
mod commands;
mod conversation;
mod domain;
mod facts;
mod ingest;
mod lifecycle;
mod llm;
mod panic_guard;
mod pipeline;
mod screen;
mod shortcuts;
mod stealth;
// `pub` (unlike this file's other top-level modules) so
// `examples/bench_rag.rs` — status.md 7.1's real benchmark — can exercise
// the actual product embedders directly instead of a reimplementation that
// could quietly drift from what `pipeline.rs` really runs.
pub mod rag;
mod storage;
mod stt;
mod transcript_coalesce;
mod transcript_log;
mod window_buttons;
mod window_fit;
mod window_memory;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Must happen before any TLS handshake anywhere in the process —
    // rustls 0.23+ needs a process-level CryptoProvider installed once, or
    // the first handshake panics ("no process-level CryptoProvider
    // available"). `reqwest` (used by the cloud STT engines and model
    // downloads) may install one lazily on its own first request, but the
    // Deepgram WebSocket engine can open a TLS connection before any REST
    // call ever happens — so this can't rely on that ordering. Verified
    // during the Phase 4 continuation research (`docs/evidence.md`);
    // `aws_lc_rs` matches the backend already in this project's dependency
    // tree (`reqwest`'s `rustls` feature resolves to it), and installing a
    // second, different backend in the same process is exactly what
    // triggers this class of panic instead of fixing it.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Before anything else: see `panic_guard`'s doc comment for the real
    // crash (recovered from `~/Library/Logs/DiagnosticReports/`) this
    // defends against — a panic in our own code running on the main
    // thread's native event-dispatch path used to force-abort the whole
    // app with no on-screen message and nothing durable to debug from.
    panic_guard::install();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(shortcuts::plugin())
        .menu(|handle| lifecycle::build_menu(handle))
        .on_menu_event(|app, event| {
            lifecycle::handle_menu(app, event.id().as_ref());
        })
        .manage(commands::AudioState::default())
        .manage(commands::SystemAudioState::default())
        .manage(commands::DeviceFollowState::default())
        .manage(crate::audio::aec::AecHub::default())
        .manage(pipeline::TranscriptionRouting::default())
        .manage(llm::AnswerTrigger::default())
        .manage(rag::indexer::RagIndexState::default())
        .manage(active_session::ActiveSession::default())
        .manage(transcript_log::TranscriptLog::default())
        .manage(window_memory::WindowMemory::default())
        .manage(conversation::Conversation::default())
        .manage(screen::cache::ScreenCache::default())
        .manage(stealth::OnScreenHidden::default())
        .manage(lifecycle::QuitDecision::default())
        // One HTTP client for the whole process, not one per question.
        // `build_provider_chain` used to call `reqwest::Client::new()` on
        // every answer, so every question paid a fresh TCP + TLS handshake
        // — real, measurable latency on the exact path this session is
        // trying to speed up. See `llm::HttpClient`'s doc comment for why
        // there is deliberately no global request timeout on it.
        .manage(llm::HttpClient::new())
        .manage(llm::LlmRuntime::default())
        .manage(shortcuts::ShortcutStatus::default())
        .invoke_handler(tauri::generate_handler![
            commands::audio::start_mic_capture,
            commands::audio::stop_mic_capture,
            commands::audio::start_system_audio_capture,
            commands::audio::stop_system_audio_capture,
            commands::audio::list_output_devices,
            commands::audio::list_input_devices,
            commands::audio::get_mic_device,
            commands::audio::set_mic_device,
            commands::audio::get_system_device,
            commands::audio::set_system_device,
            commands::audio::get_mic_capture_info,
            commands::audio::get_system_capture_info,
            commands::session::create_session,
            commands::session::end_session,
            commands::session::get_active_session,
            commands::session::set_session_instructions,
            commands::session::get_window_preview,
            commands::session::get_context_preview,
            commands::session::get_answer_rules,
            commands::session::set_answer_rules,
            commands::session::reset_answer_rules,
            commands::session::trigger_window_answer,
            commands::session::get_live_transcript,
            commands::copilots::list_copilots,
            commands::copilots::get_copilot_documents,
            commands::copilots::create_copilot,
            commands::copilots::update_copilot,
            commands::copilots::delete_copilot,
            commands::copilots::start_copilot_call,
            commands::stt::list_stt_engines,
            commands::stt::get_stt_engine,
            commands::stt::set_stt_engine,
            commands::stt::set_stt_api_key,
            commands::stt::has_stt_api_key,
            commands::stt::is_parakeet_model_downloaded,
            commands::stt::download_parakeet_model,
            commands::stt::start_channel_transcription,
            commands::stt::stop_channel_transcription,
            commands::capture::get_capture_status,
            commands::llm::list_llm_providers,
            commands::llm::get_llm_provider,
            commands::llm::set_llm_provider,
            commands::llm::set_llm_api_key,
            commands::llm::has_llm_api_key,
            commands::llm::trigger_answer,
            commands::llm::trigger_screenshot_answer,
            commands::llm::list_llm_models,
            commands::llm::get_llm_model,
            commands::llm::set_llm_model,
            commands::llm::get_llm_service_tier,
            commands::llm::set_llm_service_tier,
            commands::documents::list_doc_types,
            commands::documents::ingest_document,
            commands::documents::list_recent_documents,
            commands::documents::attach_document_to_session,
            commands::documents::ingest_text_document,
            commands::documents::delete_document,
            commands::documents::read_document_original,
            commands::job_post::fetch_job_post_text,
            commands::connections::check_model_connections,
            commands::past_sessions::list_past_sessions,
            commands::past_sessions::get_session_transcript,
            commands::past_sessions::get_session_answers,
            commands::past_sessions::get_session_context,
            commands::past_sessions::summarize_past_session,
            commands::past_sessions::search_transcripts,
            commands::past_sessions::delete_session,
            commands::overlay::open_overlay,
            commands::overlay::resize_overlay,
            commands::overlay_resize::drag_resize_overlay,
            commands::overlay::set_overlay_mode,
            commands::overlay::get_overlay_size,
            commands::overlay::hide_main_window,
            commands::overlay::show_main_window,
            commands::overlay::end_live_session,
            commands::overlay::get_overlay_prefs,
            commands::overlay::set_overlay_prefs,
            stealth::get_stealth,
            stealth::set_stealth,
            stealth::toggle_on_screen_command,
            lifecycle::request_quit_command,
            lifecycle::confirm_quit
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("resolve app data dir");
            let db =
                storage::Db::open(&data_dir.join("aside.sqlite3")).expect("open sqlite database");
            app.manage(db);

            // Hide every window from screenshots / recordings / meeting
            // shares from the first frame onward. Default is on; a saved
            // off-switch is honoured here so a previous choice survives
            // relaunch. New windows (the live overlay) pick the same flag
            // up in `commands::overlay::open_overlay`.
            let stealth_on = stealth::is_enabled_for_app(app.handle());
            stealth::apply_to_all_windows(app.handle(), stealth_on);

            // Global shortcuts (⌘↩ answer, ⌘⇧↩ look at my screen). Registered
            // here and repaired on a timer, because macOS silently drops
            // global bindings after sleep/wake — see `shortcuts.rs`'s module
            // doc comment for why these are global rather than window-local.
            shortcuts::register_all(app.handle());
            shortcuts::spawn_health_check(app.handle());
            llm::spawn_prewarm(app.state::<llm::HttpClient>().0.clone());
            // The single background consumer for the growing-summary memory
            // pipeline (`window_memory::background`) — spawned once here,
            // the same place `llm::spawn_prewarm` is.
            window_memory::init_background_worker(app.handle());

            // The red button (and Cmd+W) quits Aside at once, live window
            // included; the yellow one is Aside's own (`window_buttons`).
            if let Some(main) = app.get_webview_window("main") {
                // A window taller than the usable screen looks "cut in the
                // middle"; shrink and move it until all of it is visible.
                window_fit::fit_window(&main, 960, 640);
                window_buttons::install(&main);
                let app_for_quit = app.handle().clone();
                main.on_window_event(move |event| {
                    // Also runs on the main thread's native event-dispatch
                    // path — see `panic_guard`'s doc comment. Caught here
                    // rather than left to unwind into that path.
                    if let Err(message) = panic_guard::catch("closing the window", || {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            window_buttons::quit_now(&app_for_quit);
                        }
                    }) {
                        eprintln!("{message}");
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // The closure Tauri calls for every `RunEvent`, directly on the
        // main thread's native event-dispatch path — see `panic_guard`'s
        // doc comment for the real crash this is defending against. Caught
        // here rather than left to unwind into that path.
        if let Err(message) = panic_guard::catch("the app's event loop", || match &event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                let confirmed = app_handle.state::<lifecycle::QuitDecision>().is_allowed();
                if lifecycle::exit_needs_confirmation(*code, confirmed) {
                    api.prevent_exit();
                    lifecycle::request_quit(app_handle);
                    return;
                }
                teardown_capture(app_handle);
            }
            tauri::RunEvent::Exit => teardown_capture(app_handle),
            _ => {}
        }) {
            eprintln!("{message}");
        }
    });
}

/// Stop mic and system-audio capture. Idempotent — both `ExitRequested`
/// (confirmed quit / stealth relaunch) and `Exit` (AppKit terminate, which
/// never emits `ExitRequested`) call this, and the second call finds empty
/// slots. Leaving the CoreAudio process tap to process death is how the
/// reference app accumulated orphaned audio devices in Audio MIDI Setup.
fn teardown_capture(app_handle: &tauri::AppHandle) {
    // Disarm OS-default-device auto-follow for both channels first,
    // before tearing down capture below — a live CoreAudio listener
    // that outlives this point could, in a genuine race, still fire
    // and spawn a restart thread reaching into state this process is
    // already in the middle of tearing down. See
    // `commands::audio::disarm_device_follow`'s doc comment.
    commands::audio::disarm_device_follow(app_handle);

    let state = app_handle.state::<commands::AudioState>();
    let mut guard = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(handle) = guard.take() {
        handle.stop();
    }
    let sys = app_handle.state::<commands::SystemAudioState>();
    let mut sys_guard = sys
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(handle) = sys_guard.take() {
        handle.stop();
    }
}

/// True when `event` means this process is on its way out and Aside's own
/// teardown has to run *now*.
///
/// **Both variants are required, and matching only `ExitRequested` was a real
/// bug** that left system audio in a broken state — the "mechanical sound"
/// the owner heard — because the CoreAudio process tap was never torn down.
/// The reason is a macOS-specific path through AppKit that never emits
/// `ExitRequested` at all:
///
/// ```text
/// Cmd+Q / Aside > Quit  (OS predefined Quit, if it is ever used)
///   -[NSApplication terminate:]
///     -> applicationWillTerminate:            (tao app_delegate.rs:131)
///          -> AppState::exit()                (tao app_state.rs:272)
///               -> Event::LoopDestroyed
///                    -> RunEvent::Exit        (tauri-runtime-wry lib.rs:4185)
///     -> exit()                               <- AppKit calls this itself
/// ```
///
/// Our own Quit menu item asks first, then calls `app.exit(0)`, which
/// *does* emit `ExitRequested`. `RunEvent::Exit` is still matched in the
/// run loop so a leftover terminate path cannot skip teardown.
#[cfg(test)]
fn is_shutdown_event(event: &tauri::RunEvent) -> bool {
    matches!(
        event,
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
    )
}

#[cfg(test)]
mod shutdown_event_tests {
    use super::is_shutdown_event;

    /// The actual regression test for the bug above: before the fix this
    /// arm did not exist, so `RunEvent::Exit` — the *only* event macOS's
    /// AppKit terminate path ever produces — returned false and no teardown
    /// ran. Deleting the `Exit` arm from `is_shutdown_event` fails this test.
    #[test]
    fn the_macos_quit_path_event_is_treated_as_a_shutdown() {
        assert!(
            is_shutdown_event(&tauri::RunEvent::Exit),
            "RunEvent::Exit is what -[NSApplication terminate:] produces via \
             applicationWillTerminate:; missing it means the CoreAudio tap is \
             never destroyed"
        );
    }

    /// Guards the other direction: a routine event must not be mistaken for a
    /// shutdown, or ordinary use would kill the process. `Ready` is emitted
    /// once at startup, which is the worst possible false positive.
    #[test]
    fn an_ordinary_lifecycle_event_is_not_a_shutdown() {
        assert!(!is_shutdown_event(&tauri::RunEvent::Ready));
        assert!(!is_shutdown_event(&tauri::RunEvent::Resumed));
    }
}
