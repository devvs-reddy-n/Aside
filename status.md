# Aside status

## Legend

- **BUILT + VERIFIED** — implemented and checked with the listed commands or tests.
- **PARTIAL** — implemented, with a stated live or device-specific check still open.
- **NOT STARTED** — intentionally deferred.

## Current priorities

**PARTIAL — Windows runtime and live owner verification.** The rebuilt (2026-09-25) product now needs a real installed-app pass: permissions, two-channel capture, transcription, Command+Enter, Chat, and Screenshot with a real key. These cannot be proven from a build-only environment.

## Built product

| Area | State | Current behavior |
| --- | --- | --- |
| Windows installer | BUILT + VERIFIED | 2026-10-09: Version 0.1.5 Windows x64 executable and NSIS setup built entirely on macOS. Archive inspected: no resumes/documents/database or preview sample data. Fresh database test confirms no documents, copilots or calls. Per-user setup with WebView2 bootstrapper and checksum-pinned Microsoft C++ runtime prerequisite. Windows 10 2004+ / Windows 11; unsigned. Final file `release-windows/Aside-Setup.exe`. Built does not mean installed or run. |
| Windows runtime | PARTIAL | Windows code compiles; Windows shortcut/settings labels and minimum live layout checked with a Windows user agent in Chrome. UTM installed, no Windows guest yet; Wine installation blocked by Homebrew's Gatekeeper policy. Real microphone/speaker capture, screenshots, shortcuts, transparency, Stealth, relaunch and installer execution remain unverified. |
| Windows default audio switching | PARTIAL | CPAL default-device identities checked every 500ms by a stoppable worker. Existing device-follow restart/coalescing and pinned-device rules reused. Identity-change/removal/reconnection and worker teardown test passed on Mac; real Windows headset switch remains unverified. |
| Overall visual system | BUILT + VERIFIED | 2026-10-17: light workspace, Settings, Library and dialogs; dark live session. Capsule buttons/navigation/filter controls, rounded panels, semantic light fills and stronger focus. Settings option rows now use 85% of their previous spacing (48px to 40.8px). Immediate page mounts and restrained sheet spring. Computer checked normal and minimum layouts; root macOS app rebuilt. |
| Copilots | PARTIAL | 2026-10-14: rounded full-height setup with five AI summary bullets and full originals in Edit. Resume filename opens Preview without an eye icon; supporting files show first name plus remaining count. Model top right, transcript bottom left, Start session bottom right. Inline Replace/Select and multi-select retained. No page/text scroll at960×640,1000×650 and1280×720 with tested content; native preview/control order checked. Real capture/import owner checks remain. |
| Regular Call | BUILT + VERIFIED | Removed2026-10-14 at owner request: navigation, pages, controller, adapter, shared-guidance commands and styles. Old page links open Copilots. Retired setup kind remains readable/hidden but cannot start; saved data retained. Browser and focused Rust compatibility check passed. |
| Documents and resumes | PARTIAL | 2026-10-14: Library defaults to Resume; tabs Resume / Other / All. Each resume has Start quick call inside its three-dot menu in list/grid, using that resume and the global model/rules without JD/company or creating a Copilot. Existing audio checks/live launch reused. Native default/tab/action and preview checked; six focused Rust tests verify chosen source, fixed model, validation and atomic rollback. Actual audio/AI call remains untested. Existing upload/drop/paste/search/viewer and delete Undo retained. |
| Audio | PARTIAL | 2026-10-17: remove staged waits, finish on two sound buffers, cap meter renders, check system before mic and retry explicitly. Default macOS chime uses the built-in player; custom outputs retain routing. 81 frontend tests and 112 audio Rust tests pass (5 ignored). Built-in MacBook microphone and speakers both detected sound; AirPods microphone and system audio each detected sound individually, but repeated cold-start tests still time out despite recording permissions being enabled. Driver startup remains unresolved; no claim of fully reliable hardware checks. |
| Speechmatics live transcription | PARTIAL | 2026-10-17: direct key auth (no token-mint hop); a channel counts as started only after Speechmatics confirms (RecognitionStarted). One start per channel and the previous session fully closes first, so sessions never overlap. 20s replay buffer resends only unconfirmed audio after a drop; long silence is not reported as lost speech. Last sentence on stop is kept and saved. Slow backoff for "no free slot", stop is immediate during retries, wrong key / bad config fail once with a clear message. Filtered 48→16 kHz resampling and stronger technical vocabulary. Safe connection-state log (`speechmatics-connection.log`, no key/audio/words). 11 fake-server tests + 162 speech/pipeline tests pass. Real service with generated speech: all terms right, first words ~0.5s, sentence ~2.2s after speech ends. Real mic, AirPods, long calls and a real network drop are untested. |
| Conversation | BUILT + VERIFIED | Me and Others remain separate; finals persist to SQLite and display as a chat-style notebook. |
| Call history | PARTIAL | 2026-10-14: Delete all confirms count, excludes active calls, deletes a snapshot atomically, preserves Library/copilots; cancel and demo success checked, database preservation/rollback tests passed. Existing real-call persistence/export checks remain open. |
| Answers | PARTIAL | Answer chooses length automatically based on the question; Short/Med/Long immediately request that length for one spoken or typed answer. Empty-new-speech clicks reuse the last window; fresh speech wins, session changes clear it. Existing prompt bands override default length preferences for one reply (15–40 / 40–90 / 90–180 words, no padding). Length routing, typed retry and Auto reset tested; real cloud reply lengths remain unverified. |
| Screenshot | PARTIAL | Immediate capture feedback and visible retryable errors. Mac permission preflight, ten-second capture timeout/restoration, explicit empty/provider-stream errors. Production vision request/parser verified with synthetic local HTTP responses; real native capture and cloud reply remain unverified. |
| Memory | BUILT + VERIFIED | One growing `window_memory` summary is used by every answer path. |
| Live overlay | BUILT + VERIFIED | 2026-10-17 visual implementation: flat joined capsule length strip, separate capsule audio buttons, subdued perimeter, filled question surface, history at bottom left, compact borderless length actions and stable-width silver Answer. Computer preview checks at 440×640 and 340×260 include streaming, history, mute, follow-up, options and expanded errors. Opacity/text size/shortcuts/resizing retained. This status covers visuals; native capture/cloud verification remains in Audio/Answers. |
| Frontend structure and tests | BUILT + VERIFIED | Typed bridge, shared setup fields and kit/theme, pure call controller. 69 tests pass, including in-flight saves, failure/retry, newer edits and concurrent flushes. Production build passes with existing chunk-size advisory. |
| Settings | PARTIAL | Privacy, AI model, Speech, Strict rules (2026-10-04: below the border, no rules text, one neutral "Edit default rules" button that warns first). Shortcuts moved to the Start session panel. Options have no row dividers; main section dividers remain. Browser-checked; native persistence unchanged. |
| App shortcuts | PARTIAL | Exactly Command+Enter, Command+Shift+Enter and Command+< /> (Shift+Comma/Period) on Mac; corresponding Ctrl bindings and hints on Windows; old app bindings and hints removed. 16 Rust routing tests pass. Listed one per row at the bottom of the Start session panel (2026-10-04). Native text-editor end-of-line navigation verified; live global history routing still needs verification. |
| Model connection dots | PARTIAL | 2026-10-04: sidebar shows the default AI model and speech engine with monochrome dots and descriptive status from `check_model_connections` (OpenAI model lookup, Speechmatics token mint, Parakeet downloaded). 2 Rust tests; demo and a faked failure browser-checked. Real OpenAI/Speechmatics connection checks showed connected in the installed Mac app. |
| App bundle | BUILT + VERIFIED | 2026-10-14: app-wide refinement root Aside.app rebuilt and strictly verified as com.aside.desktop. Native setup/Library/Settings/History controls checked with saved data preserved. Windows installer not refreshed. |
| Visual design | BUILT + VERIFIED | 2026-10-14: open vertical Copilot form replace nested cards, smaller page titles and real Company/Role labels. Borderless grey fields with 18px corners; text areas use spare height and footer sits at bottom. 1000×800/650 verified with max sidebar; dark palette/Stealth gradient retained. |
| Themes | BUILT + VERIFIED | Dark monochrome workspace and editors; live window retains its black palette and independent opacity. Native window buttons retain system appearance; End/Quit and Stealth keep their red treatment. |
| Window controls | BUILT + VERIFIED | Mac close and Cmd+Q ask before quitting. Minimize leaves a half-size 48px bubble, with a soft red gradient when Stealth is on and white border when off; click restores saved visible windows. Native Mac min/restore and drag exercised. Cmd/Ctrl+H is the explicit hide/restore control. Windows native execution remains PARTIAL. |
| Window size | PARTIAL | Main fixed at 1000×800 (was 1180×860 until 2026-10-04) with resizing/maximizing disabled and custom Zoom/fullscreen guarded. Native disabled zoom verified. Existing screen-fit safety remains. Live window stays resizable. |
| Screen-share hiding / Stealth | PARTIAL | Default on. 2026-10-04: renamed Stealth, activation sweep and slow edge glow in both windows (soft inward-fading red gradient since owner direction 2026-10-14; confirmed toggles get a 420ms sweep, reversed when turning off, before saving/relaunch; Reduce Motion skips it), workspace status and compact live shield; off removes feedback, Reduce Motion is still, hidden pages pause motion. Toggle/cancel and small live layout browser-checked. Settings is the only on/off; the checkbox now asks "Turn on/off hiding?" first, then relaunches Aside via `open -n <bundle>` (fixed 2026-09-02 — the previous Tauri-built-in restart raced and could flash-and-die instead of actually reopening). While on, the main window also stays always-on-top and visible across every Space/fullscreen app, until explicitly hidden; it must not hide the window on your screen by itself. Dock icon and Aside menu-bar menus are hidden too. Activity Monitor still lists the process. Red window button quits at once; yellow hides Aside with Command+H to return while hiding is on, normal minimize when off; no green button (2026-10-04, compiled and unit-tested, real clicks owner-owed). App menu Quit asks first. Live confirmation of the relaunch fix and always-on-top is owner-owed. |

## Redesign and app discovery, 2026-10-08

- **BUILT + VERIFIED:** generated ivory A/companion-stroke logo on charcoal; shared sidebar/bubble/favicon artwork, desktop icons exported, native/sidebar browser checks and signed root Aside.app build passed. Windows icon updated; installer not rebuilt.
- **BUILT + VERIFIED:** centered confirmation dialogs above their backdrop; End-call confirmation clicked and measured.
- **PARTIAL:** owner’s 2026-10-09 screenshot still shows duplicate search rows despite one metadata/registry bundle. Targeted folder reindex and user-cache restart applied; in-place app-directory identity verified across rebuild. Visible search check remains pending.
- **BUILT + VERIFIED:** saved call engine, files and model settings preserved; demo start/answer/end paths exercised. Real audio/provider/permission checks remain **PARTIAL**.

## Foundation and architecture

- **Storage:** SQLite stores sessions, transcripts, documents, and session material.
- **Speech:** local Parakeet and cloud engines are supported by the shared pipeline. Speechmatics reconnect, region, sentence-finalization, and sample-rate recovery were hardened during live testing.
- **Audio quality:** speaker echo handling includes a loudspeaker-only AEC path and conservative transcript de-duplication.
- **LLM:** one visible answer request uses cached stable context, current window material, and the shared conversation summary. Screenshot uses a fixed vision-capable preset.
- **Documents:** PDF, DOCX, and text are parsed, stored, and supplied as session context. Retrieval experimentation was retained only where it provided a measured benefit.
- **Frontend:** React/Vite provides the main application. Tauri provides native windows, shortcuts, capture, storage, and system integration.

## Deferred or conditional work

- **Auto-answer:** the code for automatically firing an answer when a question is merely detected in speech was deleted (2026-08-31), not just disabled — the app only answers on an explicit Command+Enter, typed Chat, or Screenshot.
- **New provider work:** add it only when there is a selectable product path and a real test plan.
- **Code verification sandbox:** planned enhancement; not needed for the current desktop flow.
- **Copilots/History/Settings Stitch mockups:** the owner's Acheron Stitch project only covers Resume Library, Documents, Create Copilot, Connect, and the live Q&A view. Three `generate_screen_from_text` calls for Copilots/History/Settings mockups timed out (2026-08-31) and never produced screens; those three real pages were reskinned straight from the Acheron design system's written rules and the pattern already established by the 5 existing mockups, not from a screen-for-screen reference. Worth a follow-up owner pass in Stitch if exact mockups are wanted for those three.

## Verification standard

For every material change: compare relevant reference implementations, run `npm run build` and `cargo build`, run focused tests when behavior changes, inspect the diff, and rebuild `Aside.app`. Record only commands that actually ran in `docs/evidence.md`.

## Owner testing checklist

1. Launch the root `Aside.app`.
2. Approve Microphone and Screen & System Audio Recording when prompted.
3. Start/stop each audio source and speak while system audio is playing.
4. Confirm transcripts stay in the correct Me/Others lane.
5. Ask with Command+Enter, then Chat, then Screenshot; verify later answers use the prior conversation.
6. Test Screenshot on the current visible coding page.
