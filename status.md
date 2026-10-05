# Aside status

## Legend

- **BUILT + VERIFIED** — implemented and checked with the listed commands or tests.
- **PARTIAL** — implemented, with a stated live or device-specific check still open.
- **NOT STARTED** — intentionally deferred.

## Current priorities

**PARTIAL — live owner verification.** The rebuilt (2026-09-25) product now needs a real installed-app pass: permissions, two-channel capture, transcription, Command+Enter, Chat, and Screenshot with a real key. These cannot be proven from a build-only environment.

## Built product

| Area | State | Current behavior |
| --- | --- | --- |
| Copilots | PARTIAL | AI model can be Default (follows Settings; resolved when the call starts; 1 Rust test). Minimal look (2026-10-04): title, role and ⋯ menu on top; box-free sections (Job description with Edit and Add URL, the link field shown only after Add URL; Resume; Supporting files; Guidance); AI model, Save transcript and Start session float at the bottom right. Fits 1180×860 with no scrolling (headless-measured with long text). Four-line previews with Show more. Same editors and autosave. Real job import, autosave and start/audio flow retain their pending device checks. |
| Documents and resumes | PARTIAL | 2026-10-04: title with a grey file count under it; Paste text and Add files float at the bottom right. Merged 2026-09-26 into one Library page (All/Resumes/Documents, list/grid, "Used by N copilots", drop anywhere, paste with type choice). Earlier: rebuilt 2026-09-25 as Resumes and Documents pages: add file, drop files onto the window, paste text, search, read-only viewer (original PDF/Word or parsed text), 8-second Undo on delete. Browser preview checked with made-up data; real upload/drop/viewer owner-owed. |
| Audio | PARTIAL | Quiet audio no longer raises a warning; independent microphone/system icons glow on meter or speech activity. Browser and state tests pass; real-device timing unverified. Microphone and system audio are independently captured and transcribed; real-device confirmation remains open. |
| Conversation | BUILT + VERIFIED | Me and Others remain separate; finals persist to SQLite and display as a chat-style notebook. |
| Call history | PARTIAL | 2026-09-26: grouped by day, calls under 10 s hidden behind "Show", ⋯ menu per row, count beside the title. Earlier: rebuilt 2026-09-25: search titles and everything said (transcript hits), a call sheet with Conversation (Aside's suggestions interleaved by real time), Notes and on-demand AI summary, Text/PDF exports, delete with Undo, and a "Call ended" sheet right after a call. Preview checked; a real recorded call, the summary and the downloads are owner-owed. |
| Answers | PARTIAL | Answer is automatic; Short/Med/Long generate immediately at that length. Empty-new-speech clicks replay the last window; fresh speech wins, session changes clear it. Manual length overrides default rules for one reply. 65 window/prompt tests passed; real provider lengths remain unverified. Typed follow-up preserves failed drafts. |
| Screenshot | PARTIAL | Immediate capture feedback and visible retryable errors. Mac permission preflight, ten-second capture timeout/restoration, explicit empty/provider-stream errors. Production vision request/parser verified with synthetic local HTTP responses; real native capture and cloud reply remain unverified. |
| Memory | BUILT + VERIFIED | One growing `window_memory` summary is used by every answer path. |
| Live overlay | PARTIAL | Rebuilt minimal 2026-10-04, restyled to the main window's card, inner box and boxed controls the same day: one top bar (status dot and clock, You/Them icons, ‹ › history, options, End), the question and its answer filling the window, one bottom bar (follow-up, Screenshot, Answer). Removed: Thread/Prompter layouts, Transcript view, Short/Medium/Long, answer title and refine buttons, green accent. Text size fixed (four sizes, scales question, answer and code; browser-measured). Background 40–100% kept. Real window over real apps and native resize dragging unverified. |
| Frontend structure and tests | BUILT + VERIFIED | Typed bridge, core helpers, shared kit/theme, pure live state machine/controller and workspace pages. 60 frontend tests pass (layout and length tests removed with those features); production build passes with the existing chunk-size advisory. |
| Settings | PARTIAL | Privacy, AI model, Speech, Strict rules (2026-10-04: no rules text, one red "Edit default rules" button that warns first). Shortcuts moved to the Start session panel. Browser-checked; native persistence unchanged. |
| App shortcuts | PARTIAL | Exactly Command+Enter, Command+Shift+Enter and Command+< /> (Shift+Comma/Period); old app bindings and hints removed. 16 Rust routing tests pass. Listed at the bottom of the Start session panel (2026-10-04). Native text-editor end-of-line navigation verified; live global history routing still needs verification. |
| Model connection dots | PARTIAL | 2026-10-04: sidebar shows the default AI model and speech engine with green/red dots from `check_model_connections` (OpenAI model lookup, Speechmatics token mint, Parakeet downloaded). 2 Rust tests; demo and a faked failure browser-checked. A real check against OpenAI/Speechmatics not yet run. |
| App bundle | BUILT + VERIFIED | `./scripts/build-app.sh` produces the root `Aside.app`. |
| Visual design | PARTIAL | Minimal near-black + emerald (2026-10-04): solid near-black main window (see-through only in the live window, by its Background setting), layered surfaces, one deep emerald accent, section colours folded into one quiet look, 720px content column, even copilot toolbar. Box-free sections with hairlines on every page and in the live window, one slightly grey border around each page's sections, four-line previews, Geist font, rounder corners, no top search; only fields, the drop area, menus and buttons keep a shape (2026-10-04). Browser-checked and contrast-computed (main ≥4.5:1, live ≥7:1). Installed-app look owner-owed. |
| Themes | BUILT + VERIFIED | One fixed dark look for both windows (owner request 2026-10-03); no Appearance setting. |
| Window size | BUILT + VERIFIED | Main fixed at 1180×860 with resizing/maximizing disabled and custom Zoom/fullscreen guarded. Native disabled zoom verified. Existing screen-fit safety remains. Live window stays resizable. |
| Screen-share hiding | PARTIAL | Default on. Settings is the only on/off; the checkbox now asks "Turn on/off hiding?" first, then relaunches Aside via `open -n <bundle>` (fixed 2026-09-02 — the previous Tauri-built-in restart raced and could flash-and-die instead of actually reopening). While on, the main window also stays always-on-top and visible across every Space/fullscreen app, until explicitly hidden; it must not hide the window on your screen by itself. Dock icon and Aside menu-bar menus are hidden too. Activity Monitor still lists the process. Red window button quits at once; yellow hides Aside with Command+H to return while hiding is on, normal minimize when off; no green button (2026-10-04, compiled and unit-tested, real clicks owner-owed). App menu Quit asks first. Live confirmation of the relaunch fix and always-on-top is owner-owed. |

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
