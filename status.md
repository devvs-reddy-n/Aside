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
| Copilots | PARTIAL | Rebuilt 2026-09-25; start page ("Launchpad") rewritten again later that day: checks, optional audio test per side, remembered launch choices (listen / full view or slim bar / answers only), Start bar always visible, launch steps, inline audio recovery (audio test and launch flow preview-checked only). List + detail; a pre-call checklist that ticks only what can really be checked (speech engine, AI key/model, resume; macOS permissions shown as info); editor with details, job-post import from a link, sources (resume / supporting files, add inline), private guidance, model, save-transcript; "Save & start" saves then starts when nothing needs fixing; delete has 8-second Undo. Browser preview checked; real start-of-call flow (audio + windows) owner-owed. |
| Documents and resumes | PARTIAL | Rebuilt 2026-09-25 as Resumes and Documents pages: add file, drop files onto the window, paste text, search, read-only viewer (original PDF/Word or parsed text), 8-second Undo on delete. Browser preview checked with made-up data; real upload/drop/viewer owner-owed. |
| Audio | PARTIAL | Microphone and system audio are independently captured and transcribed; real-device confirmation remains open. |
| Conversation | BUILT + VERIFIED | Me and Others remain separate; finals persist to SQLite and display as a chat-style notebook. |
| Call history | PARTIAL | Rebuilt 2026-09-25: search titles and everything said (transcript hits), a call sheet with Conversation (Aside's suggestions interleaved by real time), Notes and on-demand AI summary, Text/PDF exports, delete with Undo, and a "Call ended" sheet right after a call. Preview checked; a real recorded call, the summary and the downloads are owner-owed. |
| Answers | PARTIAL | Typed Ask, Command+Enter, Chat, and Screenshot share the current context model. Screenshot now has a factual screen-review mode when no spoken question exists; a real-provider run remains open. A finished answer now also passes through a deterministic cleanup backstop (strips AI-tell openers/filler, trims a mid-answer cutoff) before it's shown or saved to history — see `docs/decisions.md`'s reference-repo comparison. The answer rules (2026-08-31) now explicitly classify a plain statement vs. a real question vs. a coding/architecture request before answering, and Command+Enter/Answer accepts an optional Small/Medium/Long length request from the overlay's new S/M/L buttons — both are prompt instructions only, not yet checked against a real model reply. |
| Memory | BUILT + VERIFIED | One growing `window_memory` summary is used by every answer path. |
| Live overlay | PARTIAL | Rebuilt 2026-09-25 as the Dock and rewritten again later that day (fixed-position shell, no blur layers, Rust keeps it on screen, ⋯ → Reset window size and place; answers pinned at first line, folding, refinement chips, Answers-only view, Pause/Resume, level meters). Earlier description: single scrolling feed, "Question detected" card, answer cards with code, footer that never scrolls (Answer, S/M/L, Screenshot, follow-up, End session in two taps), plain-language recovery cards (can't hear others / AI unreachable / mic), ⋯ menu (text size, background-only see-through, shortcuts), capsule mode with auto-fit height. Window may be as small as 340×260 / 340×64. Fixes the clipped End Session. Preview-verified at several sizes; the native window (docking, capsule auto-height, resize grip, dragging) is owner-owed. |
| Frontend structure and tests | BUILT + VERIFIED | `bridge/` (typed backend calls), `core/` (tiny store, question detection, answer cleanup/parse, exports), `kit/`, `theme/`, `live/` (pure state machine + controller), `home/`. `npm test` runs 42 tests; `npm run build` clean. Fixed on the way: answer cleanup used to flatten code indentation. |
| Settings | PARTIAL | Rebuilt 2026-09-25 as one page with sections: Speech (engine cards, key, Parakeet download, language), AI model (model, key, speed vs. cost), Privacy (hide from screen sharing with confirm-and-relaunch), Appearance (Match my Mac / Light / Dark), Answer rules (warning → editor → save / reset), Shortcuts. A saved key never reappears. Preview-checked for AI model; the rest owner-owed with real keys. |
| App bundle | BUILT + VERIFIED | `./scripts/build-app.sh` produces the root `Aside.app`. |
| Visual design | BUILT + VERIFIED | New system 2026-09-25 (supersedes "Acheron"): colours from the owner's reference screenshot (warm-grey page, white surfaces, one blue, calm status colours), radii 8/14/pill on a 4px grid, motion 120/200/300ms with reduced-motion respected, the Mac's system font, tokens in `src/theme/tokens.css`, building blocks in `src/kit/`. Checked in the browser preview in light and dark. |
| Themes | BUILT + VERIFIED | Match my Mac (default) / Light / Dark from Settings → Appearance; both windows follow it (the live window used to be always dark). `data-theme` is stamped before first paint. Preview-checked in both themes; installed app owner-owed. |
| Window size | BUILT + VERIFIED | Main window capped at 1600×1040 (`maxWidth`/`maxHeight` in `tauri.conf.json`) — stays resizable/maximizable but can't be dragged or maximized to fill an entire screen. |
| Screen-share hiding | PARTIAL | Default on. Settings is the only on/off; the checkbox now asks "Turn on/off hiding?" first, then relaunches Aside via `open -n <bundle>` (fixed 2026-09-02 — the previous Tauri-built-in restart raced and could flash-and-die instead of actually reopening). While on, the main window also stays always-on-top and visible across every Space/fullscreen app, so ⌘⇧H is the only thing that hides it; it must not hide the window on your screen by itself. Dock icon and Aside menu-bar menus are hidden too. Activity Monitor still lists the process. Title-bar minus and ⌘⇧H hide or show Aside on your screen. Title-bar cross and ⌘Q always ask before quitting. Live confirmation of the relaunch fix and always-on-top is owner-owed. |

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
