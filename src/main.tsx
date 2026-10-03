import { lazy, StrictMode, Suspense } from "react";
import { MotionConfig } from "motion/react";
import { TooltipProvider } from "./kit/Tooltip";
import ReactDOM from "react-dom/client";
// Styles first: colours, fonts and building blocks must load before any screen's own styles.
import "./theme/index.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "./bridge/runtime";
import { HomeApp } from "./home/HomeApp";
import { createLiveController } from "./live/controller";
import { tauriBackend } from "./live/backend";
import { LiveApp } from "./live/LiveApp";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import { applyTheme } from "./theme/theme";
import "./theme/bento.css";

applyTheme();

// Development-only: a pretend call so the live window can be checked in a plain browser tab.
const DemoLive = import.meta.env.DEV ? lazy(() => import("./live/demo")) : null;

/**
 * One bundle, two windows. The window this webview belongs to decides which
 * screen mounts: the always-on-top `overlay` window shows the live call; every
 * other window (`main`, or a plain browser tab) shows the workspace.
 */
function pickRoot() {
  if (isTauriRuntime()) {
    try {
      if (getCurrentWindow().label === "overlay") {
        return <LiveApp controller={createLiveController(tauriBackend)} />;
      }
    } catch {
      // If the window label can't be read, fall through to the workspace.
    }
    return <HomeApp />;
  }
  if (DemoLive && new URLSearchParams(window.location.search).get("demo") === "live") {
    return (
      <Suspense fallback={null}>
        <DemoLive />
      </Suspense>
    );
  }
  return <HomeApp />;
}

// Motion follows the Mac's Reduce Motion setting everywhere.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <MotionConfig reducedMotion="user" transition={{ duration: 0.18, ease: [0.22, 1, 0.36, 1] }}>
      <TooltipProvider>{pickRoot()}</TooltipProvider>
    </MotionConfig>
  </StrictMode>,
);
