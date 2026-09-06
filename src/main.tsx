import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AsideWindow } from "./window/AsideWindow";
import { OverlayWindow } from "./layers/overlay/OverlayWindow";
import { isTauriRuntime } from "./platform/runtime";
import { applyStoredTheme } from "./state/theme";
import "./styles/index.css";

applyStoredTheme();

/**
 * One Vite bundle, one `index.html`, two possible OS windows (Phase B —
 * `future refs/live-helper-ui-spec.md` point 8 in the brief: "DO NOT add
 * multi-entry Vite / a second HTML file"). Which root component mounts is
 * decided here, at startup, by which Tauri window this particular webview
 * instance is — `main` (the Create/Connect window, declared in
 * `tauri.conf.json`) gets `AsideWindow`; `overlay` (created at runtime by
 * `commands::overlay::open_overlay` when Connect is pressed) gets
 * `OverlayWindow`. A plain browser preview (`npm run dev` in a normal tab,
 * no Tauri) has no window label at all, so it falls back to `AsideWindow`
 * — the same "just show the real app shell" behavior this file had before
 * the overlay existed.
 */
/** `data-window` used to select between two divergent token overrides
 *  (light vs. dark, "Silk" vs. "Neon Glass"); as of "Cockpit Instrumentation"
 *  (2026-08-30) both windows share one token set, so this attribute is now
 *  informational only — kept for any future pass that needs to tell the two
 *  windows apart again, and because removing it would touch nothing but the
 *  attribute itself. */
function markWindowTheme(label: "main" | "overlay"): void {
  document.documentElement.dataset.window = label;
}

function pickRoot(): React.ReactElement {
  if (isTauriRuntime()) {
    try {
      if (getCurrentWindow().label === "overlay") {
        markWindowTheme("overlay");
        return <OverlayWindow />;
      }
    } catch {
      // Fall through to the default shell — a window-label lookup failing
      // should never be the reason the app fails to render at all.
    }
  } else if (new URLSearchParams(window.location.search).get("preview") === "overlay") {
    // Plain-browser-only visual QA affordance: `?preview=overlay` on the
    // `npm run dev` URL. Gated on `!isTauriRuntime()` so it can never fire
    // inside a real build — the overlay's live data (audio, transcript,
    // real screenshots) still won't work here, but its layout/styling can
    // be checked without a real call, which this window otherwise makes
    // impossible to preview.
    markWindowTheme("overlay");
    return <OverlayWindow />;
  }
  markWindowTheme("main");
  return <AsideWindow />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{pickRoot()}</React.StrictMode>,
);
