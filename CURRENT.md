# Current status

**PARTIAL: Bento restyle built and checked in the browser preview; the real live window over real apps is still owed to the owner.**

## Latest change, 2026-10-03 — Bento Modular look

The owner picked "Bento Modular" from five prototypes and asked to keep every layout and flow, changing only the look.

- **One look, light only.** Glass and dark mode are gone; Settings → Appearance is removed. Light-grey background, white tiles with soft corners, bold tight Geist type (bundled, no internet needed), one cobalt accent (#2448ff). The selected sidebar item and the selected option in switches are black with white text.
- **Live window.** The window itself is fully clear with no blur (the Mac Sidebar/Mica effect was removed in `overlay.rs`). Every control, the reading area, alerts, the question card and the menu sit on solid white tiles; Answer is the one cobalt tile; code sits on a dark tile.
- **Tile opacity.** The live menu's "Opacity" slider is now "Tile opacity", 90–100% (default 96%), saved and clamped the same way in the backend and the frontend.
- **Readability.** The live window uses darker greys, reds and blues than the main app so every line is at least 7:1 against its tile, even at 90% over a black or white screen.

Commits on `redesign-dock` (not pushed): `8568c99` glass baseline (undo point), `d4cfe18` Bento restyle, plus a docs/fix follow-up.

## Verified

- `npx tsc --noEmit`, 65 frontend tests, `npm run build` (only the old chunk-size note), `cargo build`, 800 Rust tests. Design detector: no findings.
- 47 screenshots in `~/Desktop/Aside screenshots Bento 2026-10-03/`: 25 main-window screens and states; 11 live states, each over a busy colourful background and over white.
- Scripted contrast check in the browser: live window passes 7:1 in normal, quiet-audio, offline and mic-problem scenarios, including menu and End countdown. Main app passes 4.5:1 except white text on the black selection layers, which the script can't see (separate layer); visually confirmed in screenshots.
- Found and fixed: Settings → Speech engine selected option was black on black.
- `./scripts/build-app.sh` passed; root `Aside.app` refreshed and signature verified.

## Owed (can't be checked here)

- The real live window over a real meeting app (clear window, tile readability, shadows).
- Earlier device checks still open: global shortcuts while another app has focus, real microphone/system audio, screen capture through the AI provider, native resize dragging, screen-share hiding.

## Next action

Owner opens `Aside.app`, starts a session and checks the live window over their meeting app. Known nit carried over from before: the floating "1 new" button can cover the last follow-up chip.
