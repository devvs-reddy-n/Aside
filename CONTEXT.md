# Aside

## What this is

A personal AI copilot for meetings and interviews, for the owner and a couple of friends — not a commercial product. It listens in on a call, gives you private help (suggested answers, notes, context) that only you see, and runs on both macOS and Windows from one codebase.

The name: a theatrical "aside" is a private remark only the audience hears while the scene carries on. That's the shape of what this app does for you during a call.

## Architecture

- **Tauri** (Rust backend + web frontend), one process, one app, both platforms.
- **Hidden from screen sharing** (default on): Aside stays on your display but is left out of screenshots, recordings, and meeting screen-shares (Zoom, Meet, Teams). The Dock icon and the Aside menu-bar menus are hidden too. Activity Monitor still lists the process so you can quit it yourself. Settings is the only on/off; turning it on or off saves the choice and relaunches Aside so Dock and menu-bar policy apply from the first frame. It does not hide the window on your screen. The title-bar minus and ⌘⇧H hide Aside from your screen and bring it back on the next press. The title-bar cross and ⌘Q always ask before quitting. A camera pointed at the screen still sees it.
- `src-tauri/` — Rust. Split by job: `domain/` (sessions), `audio/` (mic + speaker capture, speaking-lights, and laptop-speaker echo cancel on Me only), `stt/` (speech-to-text engines), `pipeline.rs` (live capture → transcription), `conversation/` (both ears, Prepare-only while auto-answer is off), `window_memory/` (Command+Enter conversation windows + session memory), `llm/` (one cloud answer request), `storage/` (SQLite), `commands/` (what the frontend calls: audio, session, stt, llm, documents, capture, rag).
- `src/` — React + TypeScript + Vite. The main window is a light personal-workspace UI with Copilots, Resumes, Documents, History, and Settings. It shares a soft raised/recessed surface system (`src/styles/tokens.css`, reusable pieces under `src/ui/`); the live call overlay is a separate dark, high-contrast view. Tauri provides the native windows and all real device/storage work.
- **Documents & Context** (`src/diagnostics/DocumentsContext.tsx`) is where a resume, job description, and extra notes actually get loaded into a session — before 2026-08-28 nothing in the app called any of the document backend commands, so this never happened for anyone. Loading real documents is what lets the unchanging part of the prompt clear OpenAI's prompt-cache floor; see `CURRENT.md` for the exact numbers and what's still unmeasured.
- Local SQLite (`aside.sqlite3` in the app data directory) is the notebook. Final transcripts are written as they land; a live owner click-through of quit-and-reopen is still owed.
- **Command+Enter** freezes everything spoken since the last press (finals plus the latest interim words) and sends one cloud request: strict rules + cached default documents + compressed memory + that raw window. The screen shows the answer as soon as the first words arrive. Hidden memory updates after. Auto-answer stays off. **Command+Shift+Enter / Command+K (screen)** was off through the diagnostic era; **turned on 2026-08-29 (Phase B)** — the Live overlay's Screenshot pill and its ⌘K shortcut are real, owner-requested product features (see `future refs/live-helper-ui-spec.md` §6.1/§10), so `diagnostic_screen_hotkey_may_call_llm()` now matches the Command+Enter gate instead of permanently blocking a feature this phase was asked to build. A screenshot is sent to the configured LLM provider's vision endpoint when triggered — same trust boundary as every other answer request in this app, no new provider or new data path.
- No protobuf, no cross-language wire format — it's one process, so plain JSON over Tauri's built-in command/event bridge is the whole data layer.

## Reference material

`references/Repo1-Natively` and `references/Repo2-OpenCluely` are vendored copies of two existing meeting-copilot apps (Electron/JS), kept here purely as implementation inspiration — see the standing rule in `AGENTS.md`/`CLAUDE.md`: every feature built here should be checked against how they did it, and improved on where they did it better. They are not part of Aside's own source and are gitignored.

## Non-goals

- No project wiki, weekly summaries, or process heavier than what a 2-3 person personal project needs — the context kernel (this file + `CURRENT.md` + `status.md` + `docs/decisions.md` + `docs/evidence.md`) is the only standing overhead.

## Current phase

Frontend rebuild as independent diagnostic layers. Layers 1–8 are in the sidebar. Launch opens Session Context. Command+Enter is the only answer trigger on that path. Later layers (auto-answer, a dedicated timing page, screen, polished UI) are not built yet. See `CURRENT.md` for exact status.
