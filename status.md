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
| Copilots | PARTIAL | Five separate glass tiles; AI model and Save transcript beside Start. Vertical overview fits fixed 1180×860; Content width capped at 752px; section info buttons replace help footers. Headings sit outside panels; Description/Guidance reserve five larger gray lines and open only through Edit. Readiness chips removed; extra files use Manage. Browser and native Mac appearance/reading panel verified. Real job import, autosave and start/audio flow retain their pending device checks. |
| Documents and resumes | PARTIAL | Merged 2026-09-26 into one Library page (All/Resumes/Documents, list/grid, "Used by N copilots", drop anywhere, paste with type choice). Earlier: rebuilt 2026-09-25 as Resumes and Documents pages: add file, drop files onto the window, paste text, search, read-only viewer (original PDF/Word or parsed text), 8-second Undo on delete. Browser preview checked with made-up data; real upload/drop/viewer owner-owed. |
| Audio | PARTIAL | Quiet audio no longer raises a warning; independent microphone/system icons glow on meter or speech activity. Browser and state tests pass; real-device timing unverified. Microphone and system audio are independently captured and transcribed; real-device confirmation remains open. |
| Conversation | BUILT + VERIFIED | Me and Others remain separate; finals persist to SQLite and display as a chat-style notebook. |
| Call history | PARTIAL | 2026-09-26: grouped by day, calls under 10 s hidden behind "Show", ⋯ menu per row, count beside the title. Earlier: rebuilt 2026-09-25: search titles and everything said (transcript hits), a call sheet with Conversation (Aside's suggestions interleaved by real time), Notes and on-demand AI summary, Text/PDF exports, delete with Undo, and a "Call ended" sheet right after a call. Preview checked; a real recorded call, the summary and the downloads are owner-owed. |
| Answers | PARTIAL | Answer is automatic; Short/Med/Long generate immediately at that length. Empty-new-speech clicks replay the last window; fresh speech wins, session changes clear it. Manual length overrides default rules for one reply. 65 window/prompt tests passed; real provider lengths remain unverified. Typed follow-up preserves failed drafts. |
| Screenshot | PARTIAL | Immediate capture feedback and visible retryable errors. Mac permission preflight, ten-second capture timeout/restoration, explicit empty/provider-stream errors. Production vision request/parser verified with synthetic local HTTP responses; real native capture and cloud reply remain unverified. |
| Memory | BUILT + VERIFIED | One growing `window_memory` summary is used by every answer path. |
| Live overlay | PARTIAL | Floating glass controls in the existing blue/neutral theme; company replaced by header Answer/Chat; joined Short/Medium/Long, type, screenshot and circular play action at bottom right. Light/dark and minimum/wide browser checks pass; 82–100% opacity control verified in browser; native live glass rendering unverified. 2026-09-26: icon-only mic/system controls in header; Answer shows one question/answer with arrows, Chat shows conversation; question icon opens input above compact actions. End automatically finishes after a four-second red cancel period; Cancel/Escape stops it. Browser-verified at 400×640 and 340×260. All eight resize edges/corners implemented with Mac drag tracker; native dragging remains unverified. |
| Frontend structure and tests | BUILT + VERIFIED | Typed bridge, core helpers, shared kit/theme, pure live state machine/controller and workspace pages. 65 frontend tests pass; production build passes with the existing chunk-size advisory. |
| Settings | BUILT + VERIFIED | Vertical Appearance, screen-share hiding, AI model, Speech, vertical plain shortcuts, Strict rules with larger gray preview/editor text on gray backgrounds. Rules Edit warns first; warning confirmed above editor and in native Mac. Browser checked in both themes; native persistence unchanged. |
| App shortcuts | PARTIAL | Exactly Command+Enter, Command+Shift+Enter and Command+< /> (Shift+Comma/Period); old app bindings and hints removed. 16 Rust routing tests pass. Shared UI list checked. Native text-editor end-of-line navigation verified; live global history routing still needs verification. |
| App bundle | BUILT + VERIFIED | `./scripts/build-app.sh` produces the root `Aside.app`. |
| Visual design | BUILT + VERIFIED | Shared rounded glass controls, tiles and dialogs; neutral light/dark text hierarchy, outside help and blue sidebar selection. Copilot/Settings/Library/History and minimum live size browser-checked. Native Mac opaque separated panes and vertical tiles verified; native live and Windows appearance untested. |
| Themes | BUILT + VERIFIED | Match my Mac (default) / Light / Dark from Settings → Appearance; both windows follow it (the live window used to be always dark). `data-theme` is stamped before first paint. Preview-checked in both themes; installed app owner-owed. |
| Window size | BUILT + VERIFIED | Main fixed at 1180×860 with resizing/maximizing disabled and custom Zoom/fullscreen guarded. Native disabled zoom verified. Existing screen-fit safety remains. Live window stays resizable. |
| Screen-share hiding | PARTIAL | Default on. Settings is the only on/off; the checkbox now asks "Turn on/off hiding?" first, then relaunches Aside via `open -n <bundle>` (fixed 2026-09-02 — the previous Tauri-built-in restart raced and could flash-and-die instead of actually reopening). While on, the main window also stays always-on-top and visible across every Space/fullscreen app, until explicitly hidden; it must not hide the window on your screen by itself. Dock icon and Aside menu-bar menus are hidden too. Activity Monitor still lists the process. Title-bar minus hides Aside on your screen. Title-bar cross and app menu Quit ask before quitting. Live confirmation of the relaunch fix and always-on-top is owner-owed. |

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
