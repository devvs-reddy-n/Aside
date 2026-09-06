# Aside status

## Legend

- **BUILT + VERIFIED** — implemented and checked with the listed commands or tests.
- **PARTIAL** — implemented, with a stated live or device-specific check still open.
- **NOT STARTED** — intentionally deferred.

## Current priorities

**PARTIAL — live owner verification.** The built product now needs a real installed-app pass: permissions, two-channel capture, transcription, Command+Enter, Chat, and Screenshot with a real key. These cannot be proven from a build-only environment.

## Built product

| Area | State | Current behavior |
| --- | --- | --- |
| Copilots | BUILT + VERIFIED | Reusable call setups use a compact, fixed centered editor for details, sources, guidance, call-record saving, and model choice. Saving a transcript also saves its context notes and AI summary; automatic suggestions stay off. |
| Documents and resumes | BUILT + VERIFIED | Source files and text are attached to a session and used for context; each library has a full-width header and direct upload action. |
| Audio | PARTIAL | Microphone and system audio are independently captured and transcribed; real-device confirmation remains open. |
| Conversation | BUILT + VERIFIED | Me and Others remain separate; finals persist to SQLite and display as a chat-style notebook. |
| Call history | PARTIAL | Centered history workspace with separate Conversation, Context notes, and AI Summary actions. Conversation and Context export as text or PDF; a real recorded call and OpenAI summary still need owner confirmation. |
| Answers | PARTIAL | Typed Ask, Command+Enter, Chat, and Screenshot share the current context model. Screenshot now has a factual screen-review mode when no spoken question exists; a real-provider run remains open. A finished answer now also passes through a deterministic cleanup backstop (strips AI-tell openers/filler, trims a mid-answer cutoff) before it's shown or saved to history — see `docs/decisions.md`'s reference-repo comparison. The answer rules (2026-08-31) now explicitly classify a plain statement vs. a real question vs. a coding/architecture request before answering, and Command+Enter/Answer accepts an optional Small/Medium/Long length request from the overlay's new S/M/L buttons — both are prompt instructions only, not yet checked against a real model reply. |
| Memory | BUILT + VERIFIED | One growing `window_memory` summary is used by every answer path. |
| Live overlay | PARTIAL | Audio controls, Chat, Screenshot, opacity, resize, menu positioning, and a per-ear device quick-switch (moved here from Settings, 2026-08-31) are implemented; live permission and real-device checks remain. |
| Settings | PARTIAL | Speech and AI model on one row; controls first, then at most two short description lines; themed dropdowns for speech, AI model, and speed vs. cost; saved keys hide the paste box until Change; answer-rules warning and editor are both full-window sheets; an Appearance card toggles Light/Dark. Frontend build checked. Live click-through in the installed app is still owner-owed. |
| App bundle | BUILT + VERIFIED | `./scripts/build-app.sh` produces the root `Aside.app`. |
| Visual design | BUILT + VERIFIED | "Acheron" redesign (2026-08-31), sourced from the owner's Google Stitch design system: flat monochrome black/white, 0px corners, hairline borders, frosted glass, uppercase wide-tracked labels, underline-only form fields, and a chamfered-corner accent on the app's one marquee CTA. Replaces the prior mineral-glass/violet system across Copilots, Resumes, Documents, History, Settings, and the Copilot editor. The live overlay keeps the same monochrome system inverted to a dark ground for call-time contrast. Call History, Copilots, and Documents/Resume's list tables were rebuilt on a new shared `Table`/`TableRow` primitive (`src/ui/Table.tsx`, 2026-08-31) so columns line up automatically down every row — see `docs/decisions.md`. Type (2026-09-02) is the Mac's own default system font (San Francisco via `-apple-system`) everywhere, replacing the original vendored Space Grotesk/JetBrains Mono pair; `--font-mono` still uses a real monospace family (SF Mono) for numerals/timers/code preview. |
| Dark theme (main window) | BUILT + VERIFIED | Same Acheron system, luminance inverted, selectable from Settings → Appearance (`AppearanceCard.tsx`, `state/theme.ts`) via a `data-theme` attribute and CSS token overrides in `tokens.css`. Canvas is the owner's own `#121212` reference colour. Defaults to the OS's own light/dark preference on first launch, then remembers the user's explicit choice in `localStorage` (a per-device display setting, not app data, so it skips the Rust round-trip other Settings toggles use). The live overlay is untouched — it already always stays dark. Checked in the browser preview: toggling in Settings re-themes the whole app instantly (sidebar, cards, tables, buttons checked on Copilots and Call History), and the choice survives a full page reload. Not yet checked in the installed app. |
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
