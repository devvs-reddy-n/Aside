# Evidence

This compact log keeps the evidence relevant to the current product. Detailed chronological narration was condensed on 2026-08-30.

## Latest verified baseline

The latest implementation and documentation passes successfully ran:

- `npm run build` — TypeScript and production frontend bundle completed (109 modules).
- `cargo build` — Rust application completed, zero warnings.
- `cargo test --lib` — 782 passed, 0 failed, 12 ignored. Focused: `lifecycle::*`, `stealth::tests::*`, `shortcuts::tests::hide_from_your_screen_is_a_registered_shortcut`.
- `./scripts/build-app.sh` — release bundle built in 26s (cached native stack). First strict verify lost the known iCloud-xattr race (`com.apple.FinderInfo`); retry succeeded: `valid on disk`, `satisfies its Designated Requirement`. Root `Aside.app` refreshed and signed as `com.aside.desktop`.

The build can emit existing unused-code warnings. The bundle script also strips invalid Finder metadata if necessary, signs the bundle ad-hoc, and verifies its signature.

## Current feature evidence

| Area | Evidence retained |
| --- | --- |
| Two-channel audio | Separate microphone and system-audio routes, device reporting, transcription start/stop behavior, and recovery cases were unit- and build-checked. Live hardware confirmation remains owner-owed. |
| Conversation storage | Final transcript persistence, ordering, same-speaker joining, and reload behavior were covered by focused Rust and frontend checks. |
| Context | Command+Enter, Chat, and Screenshot were traced to the same `window_memory` path; the obsolete recent-exchanges implementation and its tests were removed. |
| Documents | Multi-document context was tested with real sample inputs. PDF, DOCX, and text ingestion feed the session context. |
| Library layout | Browser preview confirmed the updated full-width Resume and Documents screens, their direct header upload action, and the absence of Grid/List controls. |
| Product-wide visual redesign | Browser preview confirmed the light workspace shell plus Copilots, Resumes, Documents, History, Settings, and the Copilot editor. The History title and padded layout were rechecked at the normal desktop preview width. |
| Mineral-glass refresh | `./scripts/build-app.sh` passed after the shared token, shell, component, library, history, document, and overlay styles were refreshed. Browser preview confirmed Copilots, Resume library, and Settings at desktop width, plus the overlay toolbar preview. Browser console contained no warnings or errors. |
| Copilot record controls | `npm run build`, `cargo build`, and two focused Copilot tests passed. Browser preview confirmed the centered editor, visible Save transcript checkbox, saved-record labels only while checked, and no automatic-suggestion control. |
| Fixed Copilot editor | Browser preview at normal desktop height confirmed all Copilot fields, model, and actions are visible in one compact, non-scrolling centered window. |
| Call history | `npm run build`, `cargo build`, and `cargo test --lib` passed (791 passed, 0 failed, 12 ignored). Focused Rust tests passed for saved two-sided transcript order and summary JSON parsing. Browser preview confirmed the centered History workspace; the preview has no saved calls, so populated dialogs and live OpenAI summary remain owner checks. |
| Overlay | Screenshot fresh capture, audio control coupling, menu position, native resizing, opacity, and collapse-to-chat recovery were built and inspected. |
| Acheron redesign (2026-08-31) | `npm run build` passed after each meaningful chunk (final pass: TS + Vite build clean, no warnings). `cargo build` passed as part of `./scripts/build-app.sh`, which produced a fresh `Aside.app` at the project root (ad-hoc signed, verified valid on disk). Browser dev-preview (`npm run dev`, plain web preview) confirmed Copilots, Resume Library, Documents, Call History, the Settings sheet, and the Create Copilot dialog against the actual Stitch "Acheron Interface" screenshots — sharp corners, monochrome palette, Space Grotesk/JetBrains Mono split, uppercase labels/buttons, and underline-only form fields all matched. Not checked: the live overlay window (unreachable outside a real call in this environment) and the window's new max-size cap in practice (config-verified only, not drag-tested). |
| Screenshot answers | Fixed vision model selection and inline explanation instructions were covered by the existing prompt tests. |
| Screenshot background review | `cargo test llm::vision_prompt::tests` passed (8 passed). The no-question prompt now requires a factual description of visible background content while retaining the prompt-injection guard. |
| Settings | Browser review confirmed the simplified Speech engine and AI model cards; real Tauri-runtime interaction remains an owner check. |
| Auto-answer removal (2026-08-31) | `cargo build` clean (zero warnings, confirming no leftover unused code after two cleanup passes) and `cargo test` (765 passed, 0 failed, 12 ignored) after deleting `maybe_trigger`, `should_consider_answering`, `cooldown_ready`, `try_claim`, `try_begin_answer`, `COOLDOWN_MS`, and `trigger::cooldown_elapsed` from `llm/mod.rs`/`llm/trigger.rs`, plus their 9 tests. Confirmed by grep that nothing outside their own now-deleted definitions still called any of them. |
| Settings rebuild + overlay device picker (2026-08-31) | `npm run build` and `cargo build` both clean. Browser preview of the overlay (`?preview=overlay`) confirmed the new per-ear device-switch arrow renders next to both the mic and speaker icons, opens a dropdown listing "Use the computer's default" (no other devices available in a non-Tauri browser preview, as expected), stays fully on screen for both ears after a left-align fix, and closes after a selection. Browser console had no errors. Speed vs. cost tier picker in the AI model card was read-reviewed against its already-tested `get_llm_service_tier`/`set_llm_service_tier` Rust commands, not exercised with a real key. |
| Settings layout (2026-09-01) | `npm run build` clean (108 modules). Speech / AI model / Speed vs. cost dropdowns are a custom `Select` (L3 frosted menu, underline trigger) so they no longer open the macOS system popup. Answer-rules warning was put back to a full-window sheet (`SettingsFullWindow`); the editor stays a full-window sheet. Themed dropdowns were kept. Live click-through in the installed app is still owner-owed. Bundle rebuild recorded with this pass. |
| Bundle | The project-root `Aside.app` has been rebuilt and launched successfully in prior implementation passes. |
| Screen-share hiding (2026-09-01) | `npm run build` clean. `cargo build` clean (zero warnings). `cargo test --lib` 773 passed, including `stealth::tests::*` and `shortcuts::tests::hide_from_screen_sharing_is_a_registered_shortcut`. Dock/menu-bar hiding added via `LSUIElement` + `ActivationPolicy::Accessory`; Activity Monitor is left listing Aside on purpose. Live Zoom/Meet/Cmd+Shift+3/Dock/menu-bar confirmation is owner-owed. |
| Stealth relaunch + hide/quit chrome (2026-09-01) | `npm run build` clean (109 modules). `cargo build` clean (zero warnings). `cargo test --lib` 782 passed, 0 failed, 12 ignored, including new `lifecycle::tests::*` (quit asks first; stealth relaunch does not; event/menu ids match the frontend). Settings Hide on/off now persists then `request_restart()`. Title-bar minus calls `toggle_on_screen` (same as ⌘⇧H). Title-bar cross / ⌘Q / Aside > Quit emit `quit-requested` and show “Quit Aside?”. Live click-through of relaunch, hide, and the quit dialog in the installed app is owner-owed. |
| Dark theme (2026-09-02) | `npm run build` clean (111 modules). Browser preview: toggling Settings → Appearance re-themed the whole app (sidebar, cards, tables) instantly on Copilots and Call History, and the choice survived a full reload. Not checked in the installed app. |
| Stealth relaunch reliability, always-on-top, confirm prompt (2026-09-02) | `cargo build` clean (zero warnings). `cargo test --lib` 784 passed, 0 failed, 12 ignored — 2 new: `stealth::tests::bundle_path_is_three_levels_above_the_binary`, `stealth::tests::a_dev_binary_outside_any_app_bundle_falls_back`. `npm run build` clean (111 modules). Relaunch now spawns via `open -n <bundle>` (falls back to Tauri's `request_restart()` only when no real `.app` bundle is found, e.g. a dev binary) instead of Tauri's own raw-binary restart, which the owner reported flashing on screen and dying within about a second. `apply_to_window` now also sets `always_on_top`/`visible_on_all_workspaces` to the hiding flag for the main window (excluding the overlay, already unconditionally always-on-top). `StealthCard` now stages the checkbox and asks "Turn on/off hiding?" before calling `setEnabled`; verified in browser preview with `window.__TAURI_INTERNALS__` stubbed so the Tauri-only card renders — clicking the checkbox opens the dialog, Cancel reverts the checkbox with no backend call, confirm text matches on/off. **Not owner-verified live**: the actual relaunch behavior, always-on-top across Spaces/fullscreen apps, and the confirm dialog inside the real installed app are still owner checks — this environment cannot open a real macOS app window. |
| Apple system font (2026-09-02) | `npm run build` clean (111 modules, CSS bundle shrank slightly with the two `@font-face` blocks gone). Browser preview: Copilots and Settings checked in both light and dark theme, plus the overlay toolbar preview (`?preview=overlay`) — headings, uppercase labels, body text, and the toolbar's numeral timer/percentage all render legibly on the fallback sans-serif this non-Mac preview environment resolves `-apple-system`/`BlinkMacSystemFont` to (confirmed via `getComputedStyle`: the CSS variables carry the intended stack). Real San Francisco rendering on macOS is an owner check. |
| Copilot editor white input boxes fixed (2026-09-02) | `npm run build` clean (111 modules). Browser preview of the Create Copilot dialog in both themes: light theme fields are a soft grey fill (was a slightly-off-white before); dark theme fields are a dark-grey fill with a light border (was a stark white box, matching the owner's screenshot). |
| Table metadata columns centered (2026-09-02) | `npm run build` clean (111 modules). Browser preview: injected a one-row table matching `CopilotTableRow`'s real markup (no saved data in this environment) and confirmed Sources/Model/Updated (header + value) sit centered in their columns, Name stayed left, Start stayed right. Same CSS pattern applied to Call History (`ss-row`) and Documents/Resume (`dc-table__row`) for consistency, not separately screenshotted (identical grid mechanism, same fix). |
| Dialog trapped inside a blurred ancestor, fixed (2026-09-02) | `npm run build` clean (111 modules). Browser preview: clicked the title bar's Quit button — before the fix this showed a cropped strip of buttons at the top of the screen (matching the owner's screenshot from the installed app); after portaling `Dialog` to `.aw-root`, it shows a properly centered "Quit Aside?" modal with title, body text, and a dimmed/blurred backdrop over the whole window. Not separately re-screenshotted: the stealth-toggle confirm, answer-rules warning, and session/Copilot dialogs, which share the same `Dialog` component and now go through the same portal. |
| Copilot editor dead space removed (2026-09-02) | `npm run build` clean (111 modules). Browser preview of Create Copilot: the empty gap between Response preferences/Save call record and the footer is gone; Job description and Private guidance are both visibly taller (matching the owner's annotated screenshot's red/blue markup); the Save call record/AI model column on the right is unchanged, still pinned to its natural compact height rather than stretching into new dead space of its own. |
| Answer cleanup backstop (2026-08-31) | `npm run build` clean (100 modules, up from 99 — the new `answerCleanup.ts`). Logic verified directly: a throwaway Node script (`node --experimental-strip-types`) ran `tightenAnswer`/`trimDanglingTail`/`cleanFinishedAnswer` against 7 opener/filler cases, 5 dangling-tail cases (including the "must NOT trim" cases: a real sentence that merely starts with "Sure enough,", a fragment with no earlier sentence boundary at all, a fragment containing a newline next to bullet lines, and an over-long tail), and an idempotence check on all 7 — all passed. Not checked: the actual behavior against a real model's output (needs a live provider call) — owner check. `cargo build` re-confirmed clean (no Rust changed this pass). Rebuilt `Aside.app`; this rebuild's signature verified on the first attempt. |
| Sidebar duplicate + drag-region button fix (2026-08-31) | `npm run build` clean. Browser preview screenshot confirmed the sidebar no longer shows a second "Aside" row — the title bar's own is the only one left. Rebuilt `Aside.app`; this rebuild's signature verified valid on the *first* attempt (no manual xattr retry needed, unlike the two rebuilds just before it). The drag-region fix itself (`-webkit-app-region: no-drag`) could not be exercised by the browser-preview tool, since that attribute only has real effect inside an actual Tauri window — owner check: confirm Minimize and Close both work correctly in the installed app now. |
| Native title bar removed (2026-08-31) | `cargo build` clean after adding `"decorations": false` to the main window in `tauri.conf.json`. Rebuilt `Aside.app`; signature re-verified valid (`codesign --verify --strict`, `Identifier=com.aside.desktop`) after the same iCloud-xattr race as the icon rebuild, resolved by hand the same way. Not checked: the actual on-screen result (a native window property, invisible to the browser-preview tool) — owner check. |
| App logo replacement (2026-08-31) | `npm run build` and `cargo build` clean after redrawing the in-app mark (`Icon.tsx`). Browser preview confirmed the new arrow mark renders correctly, at real size, in both the sidebar brand row and the window title bar, with no console errors. The new `icon.icns` was verified by hand: `codesign --verify --strict` passes, `codesign -dvvv` reports `Identifier=com.aside.desktop`, and a checksum diff confirms the bundled `Aside.app/Contents/Resources/icon.icns` matches the new `src-tauri/icons/icon.icns` exactly. Not checked: how the new icon actually looks in the Dock/Finder on a real launch (owner check). |
| Call History / Copilots table alignment fix, take one — fixed pixel widths (2026-08-31, superseded) | `npm run build` clean (TS + Vite). Visually verified with a standalone HTML file reproducing the real JSX markup and the actual `sessions.css`/`copilots.css` files with fixture rows matching the owner's own screenshot, rasterized via `qlmanage -t`. Confirmed after the fix: all columns lined up in that fixture. Superseded the same day after the owner reported a third table (Documents) with the same bug and asked for a rewrite with no hardcoded pixel widths — see the "take two" entry below. |
| Call History / Copilots / Documents table alignment fix, take two — shared-grid rewrite (2026-08-31) | `npm run build` clean (TS + Vite, 101 modules — up one for the new `src/ui/Table.tsx`). `cargo build` re-confirmed clean (no Rust touched this pass). Visually verified with a standalone HTML file reproducing the real `Table`/`TableRow` output and the real `ui.css`/`sessions.css`/`copilots.css`/`documents.css`, rasterized via `qlmanage -t` (file:// tabs are inert in the Browser pane sandbox — no screenshot/JS access). First render caught two real bugs from testing with harder fixtures than the owner's screenshots (a second Copilots row with a much longer name than any in the screenshots): a jagged/broken header background (fixed: `align-items: stretch` + per-cell `display:flex;align-items:center` in `ui.css`, with `.ss-cell-title` overriding back to `flex-start`) and the long name overflowing past its column instead of truncating (fixed: `min-width:0` on the new `.cp-table__name-copy` wrapper class). Re-rendered after both fixes: all three tables' columns line up across every row including the active/Join row, the two ended rows, a short and a long copilot name (correctly truncated with "…"), and two Documents rows with different file types/badge widths; both Name-column headers (Copilots' avatar, Documents' file icon) sit above their actual values via the invisible-spacer technique, not a guessed offset. Not checked: the real installed app with real saved sessions/copilots/documents (owner check #14, updated). Rebuilt `Aside.app`; the known iCloud-xattr signing race (`docs/decisions.md`) hit again and was resolved by hand on the first retry. |
| Question-type-aware answers + Small/Medium/Long buttons (2026-08-31) | `cargo test --lib` passed in full: 765 passed, 0 failed, 12 ignored (no count change — the new `LengthOverride`/`build_prompt_with_length` code is additive, and every existing `STRICT_RULES`/`build_prompt` test still passed unmodified, including the ones asserting exact substrings and cache-floor byte counts). `cargo build` clean, zero warnings (the compatibility wrapper `build_prompt` was gated `#[cfg(test)]`, matching the file's existing convention for `STRICT_RULES`, once its only remaining callers turned out to be test code). `npm run build` clean (TS + Vite). Browser preview (`?preview=overlay`) confirmed the three new "S"/"M"/"L" buttons render next to Answer with correct accessible labels ("Small"/"Medium"/"Long") and tooltips, and clicking one produces no console error. Not checked: any real model response — whether the AI actually classifies statements vs. questions vs. code requests correctly, and whether it actually respects a Small/Medium/Long override — since this is prompt-only guidance with no code-enforced cap (owner check #14). Rebuilt `Aside.app` twice (once before, once after the warning cleanup); both rebuilds' signatures verified valid on the first attempt. |
| Dock redesign: Rust window rules + full frontend rebuild (2026-09-25) | `cargo build` first failed at link with `ld: library 'clang_rt.osx' not found` (same failure with my edits stashed, so environment, not the change); `cargo clean -p ort-sys` then `cargo build` clean. `cargo test --lib`: 787 passed, 0 failed, 12 ignored (includes 3 new overlay-sizing tests). After adding capability permissions (`core:window:allow-start-resize-dragging`, `allow-set-focus`, `opener:allow-open-url` scoped to `x-apple.systempreferences:*`) `cargo build` clean again. `npm test` (`node --test "src/**/*.test.ts"`): 39 passed, 0 failed — live state machine (20), question detection (2), answer cleanup (5, incl. the code-indentation bug), answer error wording (4), hash router (3), pre-call readiness (5), conversation merge (2). `npm run build` clean (TypeScript strict, 97 modules; docx viewer lazy-loaded); `grep` confirmed no demo/mock data in the production bundle. Browser preview (`npm run dev`, `?demo=live` and `?demo`): live window checked at 400×640 dark and light (question card, streaming answer with key steps and a code block, follow-up chips, footer always visible), at 360×400 (End session visible, compact footer), capsule at 460 wide (question preview), the `silent` scenario (amber "can't hear the other side" card) and `offline` scenario (red banner plus a failed-answer card with Try again), the two-tap End (armed → Esc cancels → two taps ends → recap), and a script confirming no unnamed buttons/inputs. Workspace checked at the preview pane size in dark and light: Copilots (list, detail, checklist, editor dialog with footer on screen), Resumes, History with the call sheet, Settings → AI model. Found and fixed during checks: detail panel and checklist clipped (flex shrink), dialog taller than the window (backdrop grid row), sidebar text not truncating. App bundle: the first two `./scripts/build-app.sh` runs FAILED with the same `clang_rt.osx` link error, this time in the release profile (I first misread a wrapper's exit code as success); after `cargo clean --release -p ort-sys` the third run finished (`BUILD_EXIT=0`, `DONE ./Aside.app refreshed`, signed ad-hoc as `com.aside.desktop`). `codesign --verify --deep --strict` first reported extended-attribute detritus (the known iCloud-xattr race) and passed after `xattr -cr Aside.app`. Launch smoke test: ran `Aside.app/Contents/MacOS/aside` for 8 seconds; no output or panic (killed by timeout afterwards). **Not checked:** anything that needs the real app — native window docking/sizing/capsule auto-height/resize grip, real audio and permissions, screen-share hiding, real model answers, file drop, System Settings link, downloads, hiding relaunch (see `CURRENT.md` owner checks). |

## Live window + start-session rewrite (2026-09-25, later)

- `cargo build` (src-tauri): clean. `cargo test --lib`: 792 passed, 0 failed, 12 ignored (includes 5 new `window_fit` tests: fits left alone, too tall shrunk, off the right/bottom pulled back, above the menu bar pushed down, minimum size wins on a tiny screen).
- `npm test`: 42 passed, 0 failed (3 new for the remembered launch choices). `npm run build` (TypeScript strict + Vite): clean.
- Impeccable design detector on `src/live`, `Launchpad.tsx`, `AudioCheck.tsx`, `launch.css`: one finding (animating `max-height`), fixed; re-run returned none.
- Browser preview (`?demo=live`, `?demo#/copilots`): live dock at 400×640 dark and light (streamed answer with code, pinned at its first line, folded older answer, End arms to "Confirm", ⋯ menu inside the window, window size shown), 360×400, 340×260 (End and footer visible), capsule at 460 and 340 wide (found and fixed the End button being cut off at 460 by hiding the channel chips under 560px), main window 1180×860 light and 900×680 dark (bar pinned at the bottom, body scrolls), audio test (meter, "heard sound"), launch steps, Start held while a test runs. Also found and fixed in the same pass: the Answers switch cut off at 400px and the title squeezed by the status pill.
- **Not run:** anything in the real native window, real audio tests, real permission prompts, real launch failure recovery, the screen-share-hiding interaction. The hiding setting was not changed to allow screenshots.

## Simplification pass (2026-09-25, later)

- `npm test` 42 passed; `npm run build` clean; TypeScript strict clean; design detector on `src/home` and `src/kit`: no findings.
- Browser preview: start page at 1180×780 (no scroll: content 667/667), Settings at 1180×780 (no scroll: 740/740), copilot editor at 1180×780 (no scroll) and 900×680 (a 16px overflow was found, tightened by removing a textarea row and shrinking dialog padding on short windows; not re-measured after the last tweak), answer-rules editor (text box fills the dialog).
- The Rust side was not touched in this pass. Real-app checks not run.

## Inline editor, Start window, capsule removal (2026-09-25, latest)

- `npm test` 42 passed (launch-choice tests rewritten for the smaller setting); TypeScript strict and `npm run build` clean.
- Browser preview, measured by script (the browser pane was not displayed, so no screenshots): copilot form fits 1180×780 with no scrolling and has no Name field; Start session opens a 520×337 dialog with the checks and audio test; Settings fits with no scrolling, cards aligned in two equal columns. Not visually inspected this round.
- Not run: real app, real launch, audio tests.

## Styling failure fix + design review (2026-09-25, night)

- `npm run build` before the fix: esbuild warning `Unexpected "}"` at the merged CSS; brace check pointed to `src/home/launch.css:56`. After: no warning.
- `npm test` 43 passed (new stylesheet guard). TypeScript strict, `npm run build`, `cargo build` clean. Design detector on `src/home src/kit src/live src/theme`: no findings.
- Production build served with `vite preview`: body font resolves to the Apple system font, `--blue` defined, cards have borders.
- Headless Chrome screenshots of the dev preview (light and dark): Copilots, Settings, Resumes, History, live window, reviewed across three rounds. Start window and document viewer checked by script: dimmed backdrop, white panel, 18px radius; viewer 732px tall in a 780px window.
- Not run: the real app window.

## Copilot page rows (2026-09-25, night)

- `npm test` 43 passed; TypeScript, `npm run build` (no CSS warnings) clean; detector clean. Headless Chrome screenshots at 1180×780 light and 1000×700 dark; scripted checks of fit (677/677), text editor, file chooser, new form. Real app not run.

## Redesign Phase 4: screens (2026-09-26)

- **Automated checks:** `npx tsc --noEmit` clean. `npm test` 56 passed, 0 failed (new tests: guess resume from file name, History day groups and short-call rule). `npm run build` clean except the known bundle-size note, with no CSS warnings. `cargo build` clean (no Rust changes). Impeccable detector on `src/home src/kit`: one warning, the sidebar's width animation, kept on purpose.
- **Demo, scripted in the browser pane:**
  - Editing a copilot's role went pending → saving → saved, and the sidebar showed the new role.
  - A new copilot saved once it had a company, the address moved from `new` to its id, and the same page element stayed on screen.
  - Start panel: speech, AI and resume turned ok in turn, then the microphone listened, then system audio. "Checking…" cleared and Start was enabled.
- **Headless Chrome screenshots, 1100×700, light and dark:** Copilot page (also the full height and a new copilot), Library, History, Settings (full height), Start panel (mid-check and finished), Answer rules panel, file preview, saved-call window.
- **Real bug caught and fixed:** after the first save, a new copilot didn't move to its own address. React's development double-run had cleared a "still on screen" flag.
- **Not run:** the real app window, real audio checks, the System Settings round-trip, and dropping files onto the window.

## Redesign Phase 5: live window (2026-09-26)

- **Automated checks:** `npx tsc --noEmit` clean. `npm test` 56 passed. `npm run build` clean (no CSS warnings). Impeccable detector on `src/live src/kit`: no findings.
- **Headless Chrome screenshots of the live demo (`?demo=live`):**
  - 520×700 light and 400×640 dark.
  - The silent-audio scenario, with the alert collapsed and opened by focus.
  - An answer streaming while a new question card is showing.
- **Seen in those screenshots:** one 44px header row (live dot, company, clock, ⋯, End); You/Them lanes with meters; Answers/Transcript tabs; a one-line alert with its Fix button; answers at 15px; footer lengths reading "Short · Med · Long".
- **Not run:** the real window; the answer length handed from the Start panel (both windows need to share this Mac's local storage, which only the real app can show); ⌘[ ⌘] stepping with the "2 of 5" marker; the opacity slider.

## Redesign Phase 6: motion pass and acceptance (2026-09-26)

- **Automated checks:** `npx tsc --noEmit` clean. `npm test` 56 passed. `npm run build` clean (no CSS warnings). Impeccable detector on `src`: only the deliberate sidebar width animation.
- **Truncation (script at 1100×700):** the demo copilot was renamed "OneMain Financial Group Holdings International Incorporated", with a long role, and a pasted file got an 80-character name. The script checked sidebar, copilot title and role, file cards, Library, History and the palette: 9 elements on the copilot page and 10 in Library, none cut to one line.
- **Contrast (script computing ratios from the live colour tokens, light and dark):** 44 pairs, all passing after two fixes. The focus ring was 1.8:1 and is now solid accent. The faint grey was 4.42:1 on muted grey and is now 4.91.
- **Reduce Motion (Chrome with Reduce Motion emulated):**
  - Start panel: no transform at 40ms, fully visible by 150ms.
  - Keyframe animations are cut to ~0.
  - Transitions: opacity and colours only, 0.12s.
- **Screenshots:** a full "after" set of every screen in light and dark (live window at 520×700), plus the Start panel. They were compared with the Phase 0 "before" set in one side-by-side page.
- **Not run:** the real app window.

## Historical evidence retained as outcomes

- Local SQLite replaced flat or transient-only session storage.
- Speech recognition support was built for local Parakeet and cloud engines, with realtime Speechmatics configuration, reconnect, and finalization fixes.
- Document ingestion, transcript history, model selection, shortcut handling, and session context were built incrementally and checked with builds and focused tests.
- The frontend moved from diagnostic layers to the current Copilots, Documents, History, Context, Settings, and live-overlay product structure.
- Multiple visual passes converged on the current Spatial Glass system and unified icon.

## Live checks not yet evidenced

- Fresh macOS microphone and Screen & System Audio Recording permissions after the latest ad-hoc bundle signature.
- A real two-channel call through the installed app.
- Screenshot against a real visible question using an available OpenAI key.
- Screenshot against visible background content with no spoken question, using an available OpenAI key.
- A multi-turn owner check proving typed Chat reflects the growing session summary.
- A completed-call check of the separate conversation/context dialogs, both export files, and the on-demand OpenAI summary. |
- The overlay's device-switch arrow actually changing which real microphone/system-audio device is captured mid-call.
- The Speed vs. cost (Normal/Flex/Priority) tier picker actually changing OpenAI request behavior with a real key.

## Recording rule

Add only the command, test, inspection, or owner observation that actually occurred, together with its result. Do not treat a planned check as evidence.

## 2026-09-26 — screenshot-led UI finishing pass

- Startup: read CONTEXT.md/CURRENT.md; git status and diff were clean, latest commit `fb1f93c`. The newer redesign was already committed before this pass.
- `npm run build`: passed (TypeScript + Vite); existing >500 kB chunk warning remains.
- `cargo build` in `src-tauri/`: passed, 0.67 seconds.
- `npm test`: 56 passed, 0 failed, including existing stylesheet syntax guard. Repeated after the compact-window corrections; again 56 passed.
- `./scripts/build-app.sh`: completed after UI edits. Root Aside.app refreshed; codesign reported valid on disk and satisfied its designated requirement. Build log `/tmp/aside-ui-refresh-build.log`. Ad-hoc signing still carries the existing permission-reset limitation.
- Impeccable mechanical detector on changed source: one warning, the inherited sidebar width transition. Removed that transition so drag resizing responds directly. Detector was not used as a substitute for visual inspection.
- Browser demo review via the app's browser controls: Copilot light/dark, Library, History, Settings, job editor, Start panel; workspace at 1180×860 and 960×640. Edited demo company to “OneMain Financial · Customer Experience and Operations” and role to “Senior Call Delivery Analyst, Contact Center Technology and Reporting.” At the smaller size title client/scroll heights were 68/68 and 48/48; no hidden lines. Sidebar company wrapped across three lines. All edits used made-up demo state, not the owner's saved copilots.
- The first visual pass found Start's heading compressed by long metadata and the live Transcript tab clipped. Corrected panel header grouping and narrow live control spacing. Confirmed the Start heading stays on one line with full company/role below it at 960×640.
- Live `?demo=live&scn=silent`: inspected at 360×400; clicked Show details and resized to minimum 340×260. Details expanded with aria-expanded=true, closed content is inert/aria-hidden. Header bounds 11–55px, alert 57–128.4px, question card 143.4–201px, footer 201–249px: footer remains within the 260px window. At 400×640, Answers and Transcript had matching client/scroll widths of 64 and 74px; neither label was clipped.
- Browser console error query returned an empty list. `git diff --check` passed, actual source diff reviewed.
- Not checked in this pass: real native windows/calls, screen-sharing protection, live providers, real uploads/exports, or simulated OS Reduce Motion. Existing Reduce Motion CSS remains in place and disables the new disclosure movement.

## 2026-09-26 — design prompt execution and interaction verification

- Startup read CONTEXT/CURRENT and reviewed the uncommitted finishing pass; latest committed base remains `fb1f93c`.
- `npm test`: 56 passed, 0 failed. `npm run build`: passed with the existing large-chunk warning. `cargo build`: passed in 0.64 seconds.
- Browser demo at 960×640: Resize sidebar Right changed 240→250; End then Right stayed at 300; Home changed to 220. Collapsed rail inspected visually; its toggle is below the reserved native-titlebar strip.
- Live demo at 360×400: Left on Transcript selected and focused Answers; Right returned to Transcript. Empty waveform computed animation-name was `none`.
- Options menu: Tab focused Pause listening at call clock 0:37; it retained focus through 1:18, including subsequent header timer renders. Escape closed the menu and returned focus to Window options.
- At 340×260 with expanded recovery help: footer bottom remained 249px; the content panel retained its accessible name even though the tabs are hidden at this tiny height. Browser error log was empty.
- Setup links and no-match filter recovery were source-reviewed; their full error-state flows were not browser exercised. Real audio/providers and OS Reduce Motion were not tested in this pass.
- First two `./scripts/build-app.sh` runs built the release app successfully but failed strict verification when iCloud reattached root `com.apple.FinderInfo`. Direct `xattr -d com.apple.FinderInfo ./Aside.app` followed by `codesign --verify --strict --verbose=2 ./Aside.app` passed. Adjusted packaging to remove that root attribute immediately before verification and on retry; strict validation is retained.
- Reviewed actual diff; `git diff --check` passed. Available Assistant reference keyboard handling and real-meter motion inspected; named Natively/OpenCluely folders remain missing.
- The immediate root-only retry still lost the race in the next full build. Changed verification to six strict attempts, two seconds apart, deleting only root FinderInfo each time. This keeps verification strict and gives iCloud a bounded interval to settle. Final build result is in `/tmp/aside-prompt-build-fixed.log`.


## 2026-09-26 — immediate length actions and resize edges

- Removed setup Default length and all imports/call sites of the local answer-length preference. `rg` found no remaining Default length, setLength, lengthFor or handoff references in source.
- Added controller tests for all three one-response lengths followed by automatic Answer, pending/streaming duplicate-click guards, failed and ignored requests, and rejected typed requests. The first test run exposed the previously unguarded pre-stream interval; fixed it. Final `npm test`: 61 passed, 0 failed (`/tmp/aside-answer-actions-tests.log`).
- `npm run build` passed after all native resize wiring; existing large-chunk warning remains. `cargo build` passed. `cargo test --lib overlay_resize`: 3 passed, covering all eight directions, minimum sizes/opposite anchor, large sizes and negative monitor coordinates. An earlier `cargo test --lib length` ran three unrelated name matches; it is not evidence of AI answer-length quality.
- Browser demo at 400×640: Short immediately showed a streaming answer tagged Short, all answer actions disabled during streaming. A subsequent Answer produced an answer without a fixed-length tag. At 340×260, Med immediately started an answer tagged Medium. Demo content is canned and is not proof of provider length compliance.
- Minimum 340×260 with expanded audio help: all four actions and follow-up/screenshot remain visible. Footer bounds 167–249px, action heights 28px (Short/Med/Long) and 32px (Answer). All length buttons have no aria-pressed state. All eight resize areas passed DOM hit testing with the correct direction cursors.
- Workspace demo at 960×640: Answer style contains Guidance, AI model and Save transcript; Default length is absent.
- Reviewed Tauri/Tao source: native macOS resize dragging is NotSupported. Added a Mac drag tracker instead of shipping no-op handles; Windows retains its existing native command. Neither real native dragging nor real model answers was exercised. No live Aside process was available in the app inventory.
- Reference comparison: named repos absent; inspected Assistant LengthBar and WindowManager. No reference source copied.
- Final preview check: Long immediately started a response tagged Long; browser error log was empty. Temporary tabs were closed and viewport restored.
- Final `./scripts/build-app.sh` completed with exit 0. Root `Aside.app` refreshed; strict codesign verification reported valid on disk and satisfied its designated requirement. Log: `/tmp/aside-live-actions-native-build.log`. Actual diff reviewed and `git diff --check` passed. The app was not launched into a real call.


## 2026-09-26 — reported broken answer buttons: backend repair

- Startup read CONTEXT/CURRENT and checked status, diff and latest log; prior work remained uncommitted on redesign-dock.
- Traced frontend click → trigger_window_answer → respond_to_window → freeze. Confirmed the first freeze consumed speech and the next empty freeze returned an error. The old canned browser backend always supplied a question, so it could not expose this defect.
- `cargo test --lib window_memory`: 65 passed, 0 failed. New cases exercise repeated automatic and all manual lengths after consumption, new speech replacing the replay, session reset, late finals of a frozen interim, and length directives taking priority without leaking into AUTO. Tests use production window selection and prompt assembly; no cloud calls.
- `npm test`: 61 passed, 0 failed (`/tmp/aside-replay-frontend-tests.log`). `npm run build` passed with the existing chunk-size warning. `cargo build` passed in 9.76 seconds.
- Browser layout at 400×640: Short/Med/Long/Answer occupy x90–379 inside a row x21–379; Answer width 131.5px, so the group is compact and right-aligned. At 340×260, all four buttons and the follow-up/screenshot row remain visible, including the Answering state. Short started a demo response immediately; this canned response is not evidence of real-model length compliance.
- Reviewed actual Rust selection/prompt/caller changes and footer/error notice changes. Reference comparison used Assistant TranscriptStore and ChatService; no source copied. Native provider output was not exercised.
- `./scripts/build-app.sh` completed, exit 0; root Aside.app refreshed and strict signature verification passed. Log `/tmp/aside-replay-fix-build.log`. Browser console errors were empty; preview tabs closed and viewport restored. `git diff --check` passed.


## 2026-09-26 — live modes, compact controls and screenshot recovery

- Read startup context/status and reviewed checkout diff/log. Named reference directories remain absent; compared the available Assistant ChatBar, NavBar and ScreenCapture. No source copied.
- `npm test`: 64 passed, 0 failed (`/tmp/aside-live-modes-final-tests.log`). New cases cover one-question/answer navigation, latest-page selection, immediate screenshot state and visible capture failures.
- `cargo test --lib llm::openai`: 23 passed. A local HTTP test checks the actual vision request contains an image URL and exercises text success, empty DONE, streamed provider error and empty EOF. Uses synthetic data and a local server, with no real key, screen or cloud request.
- `npm run build` and `cargo build` passed. Existing Vite large-chunk warning remains.
- Browser demo: header audio icons, Answer/Chat switching, popup input above actions with focus, immediate Capturing screen feedback and one-card history (Previous showed 1 / 2 with exactly one answer). At 340×260, footer bounds were y205–249 and all controls fit inside the dock. At 400×640 the popup occupied y547–579 above controls y587–619. Browser error log empty.
- End clicked once then Cancel: session continued beyond the delay. Separate final-code test clicked End once with no cancellation; later snapshot showed Ended and Session ended. An earlier end test was invalidated by source hot reload and was not used as evidence.
- `./scripts/build-app.sh` completed with exit 0; root Aside.app refreshed and strict signature verification passed. Log: `/tmp/aside-live-modes-build.log`.
- Actual diff reviewed; `git diff --check` passed. Native capture/permission behavior and real provider response remain unverified. UI screenshot success uses the canned demo and is not evidence of actual screen capture.


## 2026-09-26 — blue glass live controls

- Startup context/status, git diff and recent commits reviewed. Existing uncommitted glass-layout work preserved; green material replaced with the requested existing blue/neutral palette.
- `npm test`: 64 passed (`/tmp/aside-glass-tests.log`). `npm run build` passed (`/tmp/aside-glass-frontend-build.log`), with the existing large-chunk warning. `cargo build` passed in 0.72 s.
- Impeccable detector run once over Header, Strip, Tray and live.css: no findings. Actual component/style diff reviewed; `git diff --check` passed.
- Browser demo: light 400×640, light/dark minimum 340×260, light wide 600×460. All footer controls fit the minimum: x25.8–317, bottom 237 within 260px. At 600×460, footer controls occupy x267.8–577, confirming right alignment. Company absent; Answer/Chat in header. Popup field opens above toolbar. End/Cancel operates. Medium immediately creates a Medium answer; play creates an auto answer. No length button has aria-pressed. Browser error log empty.
- Theme restored to Light after the temporary dark preview. These are browser/demo checks, not native glass or real-provider validation. No cloud calls or native audio sessions run.
- `./scripts/build-app.sh` completed with exit 0; root Aside.app refreshed and strict signature verification passed after the existing iCloud metadata retry. Log: `/tmp/aside-glass-app-build.log`. Preview tabs closed and viewport reset.


## 2026-09-26 — refined live UI and activity indicators

- Startup context, current status, git status/diff/log reviewed. Impeccable craft guidance reused; compared available Assistant Waveform real-level/muted behavior. Named comparison repositories remain absent.
- `npm test`: 65 passed, 0 failed (`/tmp/aside-audio-glow-tests.log`). Added independent-channel activity coverage including speech-only activity, quiet samples and stale values on muted/starting/failed channels. Silence reports listening with no notice.
- `npm run build` passed (`/tmp/aside-audio-glow-web-build.log`); existing large-chunk warning remains. `cargo build` passed in 0.72 s. Design detector: no findings. Actual diff reviewed and `git diff --check` passed.
- Browser demo at 400×640: silent system audio produced zero alerts; microphone alone had data-active and blue glow while system remained neutral. Muting mic removed its activity. Computed reading background alpha is 0.99.
- Browser demo at 340×260: system alone glowed after muting mic; footer stayed inside the dock, bottom 249px. Blue palette and compact joined actions retained. These synthetic audio checks do not establish native microphone/system activity timing.
- Mic-failure browser scenario still showed recovery help and no false mic glow; browser error log empty. Preview closed and viewport restored. `./scripts/build-app.sh` completed with exit 0; root Aside.app refreshed and strict signature verification passed after the existing iCloud metadata retries (`/tmp/aside-audio-glow-app-build.log`).


## 2026-09-26 — compact Settings and five-line rules preview

- Read startup context/status and checked git status/diff/log. Compared official Apple grouped-list guidance and available Assistant settings CSS; requested named reference directories remain absent. Reused Impeccable craft guidance.
- `npm test`: 65 passed (`/tmp/aside-settings-tests.log`). `npm run build` passed (`/tmp/aside-settings-web-build.log`) with the existing chunk warning. `cargo build` passed in 0.60 s. Detector run once: no findings. Actual diff reviewed; `git diff --check` passed.
- Browser at 1180×822 light: all groups visible. Initial 960×640 dark pass exposed excess height with the Speechmatics missing-key field; fixed spacing as one batch. Final 960×640: page client/scroll height both 588px. Final 836×822 light: both 770px, horizontal client/scroll width both 616px. All six sections and all seven shortcuts remain visible.
- Rules preview height 90px with 18px line height (five lines). Edit top at y130 precedes preview y197; clicking it opened the full Answer rules textbox. Done closed it. Theme restored to Light. Browser error log empty. These are demo checks, not native settings/key persistence tests.
- Final `./scripts/build-app.sh` completed with exit 0; root Aside.app refreshed and strict signature verification passed after the existing iCloud metadata retry (`/tmp/aside-settings-app-build.log`). Final responsive 960×640 check still had client/scroll height 588px. Preview closed and viewport restored.

## 2026-09-26 — UI consistency and shortcut cleanup

- `npm run build` passed (existing bundle-size advisory remains).
- `npm test`: 65 passed.
- `cargo build` passed.
- `cargo test shortcuts::tests --lib`: 16 passed, including only-four registration definitions, arrow parse/dispatch and rejection of removed keys.
- `cargo test lifecycle::tests --lib`: 5 passed.
- `git diff --check` passed; reviewed shortcut/menu routing, removed frontend listeners, shared labels, Settings structure, color tokens and page CSS.
- `impeccable detect src/home src/live src/theme src/main.tsx`: exit 0, no output/findings (`/tmp/aside-consistency-design-scan.log`).
- Browser inspected light Settings at 1180×822, dark at 960×640 and narrower light at 836×822. Detected General overflow at 640px height, then moved it to the right column. Final 960×640 with empty Speechmatics key input: page/scroll heights 588/588, all six groups visible, no row overflow. Rules preview height 90px (five 18px lines); Edit opens full editor. Help DOM nodes lie outside control cards.
- Copilot/Library/History visual inspection: common neutral groups and readable metadata; selected sidebar labels white on RGB(49,88,217). Sidebar Search opens searchable content after removing its old shortcut.
- Live browser at 400×640 and 340×260: controls visible, four shortcut hints, Short generates a demo answer. No console errors. Does not validate real global keys, capture/provider calls, native menu clicks or native audio.
- `./scripts/build-app.sh` completed successfully; root `Aside.app` refreshed, signature valid on disk and designated requirement satisfied. Log: `/tmp/aside-consistency-app-build.log`.


## 2026-09-26 — shared glass and Copilot tiles

- Reviewed current state and existing diff; compared available Assistant WindowManager native-vibrancy/fallback approach. Both requested reference repositories remain absent.
- `npm run build` and `cargo build` passed. `npm test`: 65 passed (`/tmp/aside-tiles-tests.log`). `cargo test commands::overlay::tests --lib`: 8 passed, including size and opacity bounds.
- `impeccable detect src/theme/glass.css src/home/pages/CopilotPage.tsx src/kit/RichText.tsx`: exit 0, no findings (`/tmp/aside-tiles-design.log`). Reviewed actual tile, RichText, material and native-window changes.
- Browser: light/dark Copilot 1180×860; long description at 960×640 had page client/scroll heights 588/588, all five tiles visible. Centered description arrow opened all 14 test bullets in a reading panel. Corrected wide header wrapping, then confirmed model/transcript/Start all aligned at y56. Resume Preview accessible label checked.
- Settings 960×640 client/scroll heights 588/588. Shared material inspected on dark Library/History. No browser errors.
- Live 400×640 and 340×260 inspected. Minimum footer spans x29.8–321 and ends y241 inside the 340×260 viewport; dock inset 6px. Opacity slider changed from 82 to 100 and computed background became opaque RGB(29,29,31). No browser errors. Demo only, no real audio or provider call.
- First rebuilt Mac bundle launched after quitting the idle prior version. Native main-window frost, rounded frame, five independent tiles and header controls visually inspected with saved content. Native description arrow opened full text. No saved content changed; no real session started. Windows material and native live-window appearance untested.
- Final `./scripts/build-app.sh`: exit 0, root Aside.app refreshed and strict signature verification passed after the existing iCloud metadata retry (`/tmp/aside-glass-final-build.log`). Existing Vite chunk-size advisory remains. Reduced-transparency specificity fix is in this final bundle; ordinary native appearance had already been checked in the preceding build.
- Final post-build strict verification hit the known iCloud FinderInfo reattachment; removed only that bundle metadata and repeated deep/strict verification successfully. Final `git diff --check` passed. Temporary preview tabs closed, viewport reset and dev server stopped.


## 2026-09-26 - opaque vertical layout and punctuation shortcuts

- Startup context/status and git status/diff/log reviewed. Reused Impeccable session context, read layout/craft-floor, applied relevant Design Taste refinement guidance. Named reference repos absent; read available Assistant WindowManager/hotkeys patterns.
- `npm run build` passed (existing large-chunk advisory); `cargo build` passed. 65 frontend tests passed (`/tmp/aside-opaque-tests.log`); 16 shortcut tests and 5 lifecycle tests passed. Old Command+Arrow combinations rejected by dispatch tests; punctuation bindings parse/round-trip.
- Detector returned no findings (`/tmp/aside-opaque-design.log`). Reviewed actual changes to previews, Settings order/warning, dialog portal/layering, window config/menu guards and shortcut definitions; `git diff --check` passed.
- Browser first pass found inherited title flex-basis inflating vertical header; removed it and tightened tile spacing. Confirming clean load at 1180×860 with long company title: Copilot page/scroll heights 792/792, all five tiles share x262. Long synthetic Description/Guidance previews measured 90px with 18px lines and clamp 5, no expand controls. User native content was not edited.
- Light/dark Settings inspected. Section DOM order matches requested order; plain shortcut text contains Command+< and >. Seven-word warning opens; Continue opens editor. Reset confirmation visibly above editor; Cancel returns to it. Browser error log empty.
- Final root Mac bundle relaunched after quitting idle prior version. Real saved Copilot content shows opaque raised panes, vertical tiles and five-line previews. AX reports zoom disabled. In real description editor, Command+Right then Command+Shift+Left selected the visual line, demonstrating native text movement; Command+Left also sent with no history activation. No text edited. Native Strict rules warning shown in foreground and dismissed.
- `./scripts/build-app.sh` final exit 0; root Aside.app refreshed and strict signature verification passed after known iCloud metadata retries (`/tmp/aside-opaque-final-build.log`). Windows and global punctuation history navigation during a live session not exercised. No actual audio/provider calls. Temporary browser tab closed and viewport reset.


## 2026-09-26 - heading and typography refinement

- Inspected supplied screenshot, current components/styles, current status and git state. Reused prior skill context and re-read craft-floor. Named comparison repositories remain absent.
- `npm run build` and `cargo build` passed. No new behavior tests for this presentation-only change. `git diff --check` and targeted design detector passed without findings.
- Browser 1180×860: all five headings measured outside their panel, each heading bottom 4px above panel top. Copilot and Settings page/scroll heights both 792/792. Light previews gray RGB(98,98,103); dark editor RGB(165,165,170). Rules preview 13px; full editor 15px. All shortcut entries share x317 and progress vertically at y496/518/540/562. Light Copilot and dark Settings visually inspected; rules warning and editor open successfully.
- Final `./scripts/build-app.sh` completed with exit 0, root Aside.app refreshed and strict signature verification passed (`/tmp/aside-type-final-build.log`). Browser errors empty; temporary tab closed and viewport reset. This typography pass was browser-verified; the new bundle was not relaunched.


## 2026-09-26 - narrower panels, gray fields, section info

- Inspected current state, supplied screenshot, diff and shared Popover/editor components. Named comparison repositories remain absent; checked available Assistant settings CSS.
- `npm run build`, `cargo build`, `git diff --check` and targeted design detector passed. Presentation-only changes use existing Popover behavior; no new tests added.
- Browser 1180×860: Copilot panel and Settings grid width 752px; both page/scroll heights 792/792. Copilot has no helper footers and five info buttons. Each opened its corresponding explanation and Escape closed it; Guidance retained the Answer rules link.
- Dark Copilot gray fill RGB(41,44,52); light Strict rules preview/editor RGB(230,232,237). Full rules editor padding 12px/14px. Visually checked dark Copilot/help popup and light rules editor. No user data edited.
- `./scripts/build-app.sh` completed with exit 0; root Aside.app refreshed and strict signature verification passed (`/tmp/aside-info-app-build.log`). Browser error log empty; tab closed and viewport reset. Native bundle not relaunched for this presentation pass.


## 2026-10-03 - Bento restyle

- `npx tsc --noEmit` passed; `npm test` 65 pass / 0 fail; `npm run build` passed (existing chunk-size advisory only); `cargo build` passed; `cargo test --lib` 800 passed / 0 failed (default-opacity test updated 82 → 96 to match the new spec). Impeccable detector on `src`: `[]`.
- Headless Chrome screenshots (dev server, 2x): 25 main screens and 11 live states, live ones over a busy CSS background and over white, in `~/Desktop/Aside screenshots Bento 2026-10-03/` (47 files). Reviewed via contact sheets. Found the Settings engine switch showing black text on its black selected pill; fixed (`.set .seg__item[data-state=on]` now white) and re-shot.
- In-browser contrast script (text colour vs. stacked backgrounds, composited over black and over white): live window, scenarios normal / silent / offline / mic, after Answer, Answer tab, menu open and End armed — zero lines under 7:1 after darkening live greys, danger/accent inks, code comments and solid alert chips. Main routes (copilot, new copilot, library, history, call detail, settings) — zero under 4.5:1 apart from white text on the black selection layers, which the script cannot see; screenshots confirm those read white on black.
- Reduced motion: global `prefers-reduced-motion` rule in `base.css` unchanged; new hover lift and audio ring transitions have their own reduce guards. Not re-run with emulation this pass.
- Reference check: read the reference overlay's transparent-window card styling and frost slider (`references/src/renderer/src/styles/base.css`, `GlassOpacity.tsx`); no source copied.
- `./scripts/build-app.sh` exit 0; root Aside.app refreshed; `codesign --verify --deep --strict` passed after the known iCloud metadata retry. The new bundle was not launched; the real live window over real apps is owner-owed.


## 2026-10-03 - pastel look and live rebuild

- `npx tsc --noEmit` passed; `npm test` 68 pass / 0 fail (new: launch-prefs layout parsing and old-note migration, Thread stepping in place, layout change clears the cursor); `npm run build` passed (existing chunk-size advisory only); `cargo build` passed; `cargo test --lib` 800 passed / 0 failed (opacity floor 90 → 40 and default 96 → 92 updated in the two spec tests). Impeccable detector: one finding, `side-tab` on the sidebar selection edge, kept deliberately (owner's reference).
- Headless Chrome (dev server, 2x) into `~/Desktop/Aside screenshots Pastel 2026-10-03/`, 91 files: 25 main screens/states at 1180×860; live 11 states × Focus/Thread/Prompter × 100%/50% over a busy CSS background. Reviewed via contact sheets. Fixes made from the review: Start session wrapping (content width 820 → 880), truncated model name, Prompter action row overflow and duplicate icon, Prompter showing a waiting question too small, tiny-window squeeze (You/Them moved into the top bar), narrow slim bar (S/M/L labels).
- In-browser contrast script (text vs. stacked backgrounds; gradients tested at each most-opaque stop; composited over black and white): live window at 100% — Focus, Thread and Prompter × normal/offline/mic/silent, after Answer, transcript, menu and End armed — minimum 7.01, zero failures, after deepening the live Answer gradient, armed End, danger/warning inks, off-lane fill, selected layout label and grey text. At 50% minimum 1.44, at 40% minimum 1.08 (worst case, halo not measured). Main app (copilot, new copilot, Library, History, call detail, Settings) — zero under 4.5:1 after deepening the main gradient (Start session was 4.44).
- Reduce Motion: global `prefers-reduced-motion` rule in `base.css` applies to all new animations; not re-run with emulation.
- Reference check: read the reference overlay's component list (`references/src/renderer/src/windows/overlay/Overlay.tsx` and siblings); no code copied.

## 2026-10-04 - minimal near-black + emerald, window buttons

- `npx tsc --noEmit` passed; `npm test` 68 pass / 0 fail; `npm run build` passed (chunk-size advisory only); `cargo build` passed; `cargo test --lib` 803 passed / 0 failed (3 new in `window_buttons`: yellow hides with hiding on, minimizes with hiding off, Command+H is recognised and Command+Shift+H is not).
- Browser preview (dev server, demo data) at 1180×860: copilot, Settings, Library, History reviewed by screenshot. Fixes from the review: Settings was still on the old wide column; Replace was a second green button (now plain); Settings model picker cut off the name (widened); speaking lanes in the live window had a loud green ring (now a thin edge).
- Live window at 440×720: Focus at 100% before and after pressing Answer, Thread at 60%, reviewed by screenshot.
- Contrast computed from the token values, composited over white and black: main card text-2 8.89, text-3 6.26, emerald ink 9.51; white on the main green button 5.0 (lightest stop); live at 100% text-2 9.83, text-3 7.68, text-3 on the answer card 8.01, white on the live Answer button 7.39. The earlier in-browser per-element checker was not re-run.
- No em dashes in UI strings (grep of `src`; only a regex character class matches).
- Not run: the native window buttons, Command+H and the see-through tint in the installed app; these need the owner.

## 2026-10-04 - minimal live window

- `npx tsc --noEmit` passed; `npm test` 60 pass / 0 fail (removed: 3 launch-prefs tests, 3 length tests, 2 layout/thread tests; rewritten: double-press guard, failed-request release, history stepping, screenshot); `npm run build` passed; `cargo build` passed (no Rust change in this step).
- Browser preview (demo, 420×560): waiting question, streamed answer with list and code, window options open, mic-failure alert at 55% over a striped background. Screenshots reviewed.
- Text size measured in the browser: answer 15px → 17.3px (Medium) → 23.1px (Extra large); question and code scale with it; the selected size shows as checked.
- CSS class audit: every `lv-` class used in `src/live` is defined in `live.css` and every defined one is used.
- Contrast from token values at 100%: lightest grey text (#b2b6bc) 8.0:1 on the panel top, 9.0:1 at the bottom.
- Not run: the real live window in `Aside.app`.
