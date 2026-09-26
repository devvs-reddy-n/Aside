# Current status

**PARTIAL (builds and tested; owner real-device pass still owed).** Aside is a desktop meeting copilot: a normal workspace window plus a compact, always-on-top live window. It captures microphone and system audio separately, transcribes them, keeps a growing session context, and answers spoken, typed, and screenshot questions. It is hidden from screenshots, recordings, and meeting screen-shares by default (still needs a live owner check against Zoom and Cmd+Shift+3).

## Latest change (2026-09-25, night): styling restored

The installed app had lost all its styling because of one stray `}` in a stylesheet (details in `docs/decisions.md`). Fixed, guarded by a new test, checked on the production build, and followed by a design review pass across the workspace and live window. Real-app look still needs the owner's eye.

## Latest change (2026-09-25, newest): edit on the page, Start in a small window

Right pane of Copilots is the edit form; **Start session** opens a small window (checks, optional audio test, Start). No listen/open-as options; both sides always start (toggle them in the live window). Slim bar removed everywhere. Settings redone as aligned cards of rows. Preview-measured only (see `docs/evidence.md`); Rust still registers the old ⌘H shortcut, now ignored.

## Latest change (2026-09-25, latest): simpler, one-screen pages

Start page cut down to the essentials; copilot editor and Settings are each one screen with no sub-pages (import-from-link is first in the editor); the answer-rules editor fills its window; wording shortened everywhere; the Flex/Priority speed-vs-cost choice was removed (the app always uses the regular lane; a few cents per interview either way). Preview-checked only; see `docs/decisions.md` and `docs/evidence.md`.

## Latest change (2026-09-25, later): live window and start page rewritten from scratch

The owner kept seeing the start-session page and the live window "cut in the middle", so both were deleted and rewritten (branch `redesign-dock`, not committed). The cut-off could not be reproduced in the browser preview and the real window is hidden from screenshots by design, so the rewrite removes everything that could behave differently in the Mac's web engine and adds a rescue: the ⋯ menu shows the window's size and has **"Reset window size and place"**. Rust now also keeps the live and main windows fully on screen. If it is still cut off in the real app, please send a screenshot (hiding blocks the built-in screenshot tool; turn hiding off in Settings → Privacy for a moment to take one).

- **Start page ("Launchpad")**: setup summary, before-you-start checks, an optional **audio test** for each side (a few seconds, live meter, records nothing), **how to begin** (listen to both / just me / just others, open as full view or slim bar, answers only), a **Start bar that never scrolls away**, step-by-step launch progress, and audio trouble handled inline (Open settings / Try again / Start anyway).
- **Live window**: answers stay at their first line while streaming; older answers fold; each answer shows its question; refinement chips; "Answers only" view; sticky S/M/L; Pause/Resume listening; level meters; "Latest" jump button; two-tap End in the header; menu inside the window.
- Verified in the browser preview and by tests only (see `docs/evidence.md`); the native window, real audio tests and permission flows are owner-owed. Checks below still apply, plus: try the audio test on both sides; start with "Slim bar"; shrink the live window as small as it goes and confirm End and the footer stay visible; use ⋯ → Reset window size and place.

## What changed on 2026-09-25 (branch `redesign-dock`, not committed)

The whole frontend was rebuilt from scratch; the Rust backend was left as it was, except for the live window's sizing rules. The owner chose **direction A "Dock" + E's "Question detected" card + B's capsule** from five prototypes, and asked that colours follow the reference screenshot. The app is still named **Aside**.

- **The clipped-End-Session bug is fixed at its root.** The old live window had a 12-button single row that needed 772px in a window that could never be narrower than 720px, so End Session could sit off-screen. The live window now can be as small as 340×260 and its footer (Answer, length, Screenshot, follow-up, End session) never scrolls away. Verified at 400×640, 360×400 and 460×capsule in the browser preview.
- **Live window ("Dock")**: one scrolling feed of what was said, "Question detected" cards (Answer this / Not now), and answer cards that open at their first line, with code shown as code. Recovery cards in plain words: can't hear the other side (macOS permission / AirPods, with "Open System Settings" and "Check again"), can't reach the AI service (with "Retry now"), microphone not working. End session is two taps in place (a bar drains over 4 seconds; Esc keeps going). ⋯ menu: text size, window see-through (background only, text stays sharp), shortcuts list. A **capsule** mode shrinks the window to one bar and grows a preview under it when a question or answer arrives.
- **Workspace**: Copilots (list + detail with a pre-call checklist that only ticks what can truly be checked, editor with job-post import from a link, sources, guidance, model), Resumes and Documents (add file, drop files onto the window, paste text, read-only viewer), History (search titles and everything said, conversation with Aside's suggestions in time order, notes, AI summary, text/PDF export), Settings (Speech, AI model, Privacy, Appearance, Answer rules, Shortcuts). Deleting anything is undoable for 8 seconds. A "Call ended" sheet appears after a call.
- **Real bug found and fixed on the way:** the answer-cleanup step used to collapse runs of spaces everywhere, which flattened code indentation (Python answers would break). It now leaves fenced code alone (`src/core/answerCleanup.ts`, with tests).
- **Permissions added** (`src-tauri/capabilities/default.json`): resize-by-dragging and focus for the live window, and opening System Settings' Privacy panes (previously the resize grip and any such link were silently blocked).
- **Build environment fix (no code change):** `cargo build` had started failing at link time with `library 'clang_rt.osx' not found` because a cached build step (`ort-sys`, the speech engine's library) remembered a removed Xcode-beta path. Fixed by `cargo clean -p ort-sys` (dev) and `cargo clean --release -p ort-sys` (the app bundle build).

## Product state that did not change (backend)

- **Hidden from screen sharing:** default on; Settings → Privacy is the only on/off and it relaunches Aside (asks first). Stays on top of other windows while on. ⌘⇧H hides Aside from your own screen; ⌘Q and the top-bar cross ask before quitting. A camera pointed at the screen still sees it.
- **Answers:** Command+Enter / Answer, typed follow-up, and Screenshot all use the same `window_memory` context path. S/M/L length is a prompt instruction, not a hard cap. A finished answer passes through a deterministic cleanup (strips "Certainly,"-style openers and filler, trims a dangling half-sentence). Auto-answer does not exist.
- **Question detection is new and lives in the frontend** (`src/core/questions.ts`): a light guess that only decides whether to show the "Question detected" card. It never answers by itself.
- **Speech:** local Parakeet or cloud Speechmatics. Parakeet's one-time download shows no progress (the backend reports none).
- **Global shortcuts** (work while another app is focused): ⌘↩ answer, ⌘K screenshot, ⌘⇧A follow-up box, ⌘H shrink/expand, ⌘[ ⌘] previous/next answer, ⌘⇧H hide from your screen.
- **Distribution:** the project-root `Aside.app` is the installed-style bundle (`./scripts/build-app.sh`).

## Verified state (2026-09-25)

`./scripts/build-app.sh` produced the root `Aside.app` (signed ad-hoc; it also needed `cargo clean --release -p ort-sys` first, same stale-Xcode-path cause; macOS will ask for permissions again after this rebuild); the app starts without a crash (8-second launch smoke test). `cargo build` clean; `cargo test --lib` 792 passed / 0 failed / 12 ignored (includes overlay-sizing and 5 window-fit tests); `npm test` 42 passed (live state machine, question detection, answer cleanup, error wording, hash router, pre-call readiness, conversation merge); `npm run build` clean (TypeScript strict + Vite). The browser preview (`?demo`) was used to check the live window (dark and light, 400×640, 360×400, capsule at 460 wide, the silent-audio and offline scenarios, the two-tap End and Esc cancel, streamed answers with code) and the workspace pages (Copilots, editor, Resumes, History with the call sheet, Settings, light and dark). See `docs/evidence.md`.

**Not verified here (needs the real app):** the actual native live window (docking to the top-right, capsule auto-height, the resize grip, dragging), real audio and permissions, screen-share hiding, real model answers, file drop onto the window, opening System Settings, exports downloading, the relaunch after changing hiding, and everything that needs a real OpenAI/Speechmatics key.

## Owner checks still needed

1. Open the root `Aside.app`; approve Microphone and Screen & System Audio Recording when macOS asks.
2. Copilots → pick one → "Start session". Check the live window docks at the top-right at about 400×640, and that **End session is always visible**, including after you drag the window smaller (try as short as you can go).
3. Speak (mic) while system audio plays: the lines should land under "You" and "Others". Ask a question from the other side; the "Question detected" card should appear. Press **Answer this** (or ⌘↩).
4. Try ⌘K (screenshot answer), the S / M / L buttons, a typed follow-up, and "Shorter" / "Add an example" on an answer.
5. Turn "Others" off, then back on. Deny system-audio permission once (System Settings) to see the amber "can't hear the other side" card; "Open System Settings" should open the Screen & System Audio Recording pane.
6. Shrink to the capsule with the button next to ⋯ (or ⌘H). It should become one slim bar; trigger a question/answer and confirm the preview grows under it and the window height follows it. Expand again and confirm the old size returns.
7. Drag the live window by its header; double-clicking the header must **not** maximize it. Drag the lower-right corner to resize.
8. End session with two taps. The live window closes and a "Call ended" sheet appears in the main window; open it in History and check Conversation (with Aside's suggestions in order), Notes, AI summary, and the Text/PDF downloads.
9. Delete a copilot, a resume and a call; each should vanish and offer Undo for 8 seconds. Press Undo on one.
10. Resumes/Documents: add a PDF and a .docx with the button, then by dropping onto the window; open each in the viewer (Original file and Parsed text); paste text.
11. Copilot editor: import a job post from a real link; attach a resume and a document; "Save & start" (saves, then starts the session straight away when nothing needs fixing; otherwise it saves and points at what to fix).
12. Settings: change speech engine (and the Parakeet download if using it), save/replace a key (a saved key should show "Saved" and never reappear), speed vs. cost, Light/Dark/Match my Mac, answer rules (warning → editor → Save / Reset). Turn hiding off and on: confirm the dialog and that Aside reopens (does not flash and vanish).
13. With hiding on, confirm Aside does not appear in a Zoom/Meet share or a ⌘⇧3 screenshot.
14. With a real key, ask several questions and check no answer opens with "Certainly,"/"Sure," and Python answers keep their indentation.
15. Set the live window's text size and see-through slider (⋯ menu) and confirm text stays sharp when the background fades.

## Next action

Owner runs the checklist above on `Aside.app`. Anything wrong in the native window (sizing, docking, permissions) comes back as a fix here; the browser preview cannot show it.
