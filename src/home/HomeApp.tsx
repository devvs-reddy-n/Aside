import { AnimatePresence, motion } from "motion/react";
import { useEffect } from "react";
import { Callout } from "../kit/Bits";
import { Button } from "../kit/Button";
import { EASE_OUT, BASE } from "../kit/motion";
import { ToastHost } from "../kit/toast";
import { useStore } from "../core/store";
import { isTauriRuntime, isDemoMode, NOT_TAURI_MESSAGE } from "../bridge/runtime";
import { CallSaved, announceCallEnded } from "./CallSaved";
import { QuitGuard } from "./Chrome";
import { CommandPalette } from "./CommandPalette";
import { CopilotsPage } from "./pages/CopilotsPage";
import { HistoryPage } from "./pages/HistoryPage";
import { LibraryPage } from "./pages/LibraryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { refreshSystem, returnToCall, systemStore, watchSystem } from "./data";
import { useRoute } from "./router";
import { Sidebar } from "./Sidebar";
import "./home.css";
import "./launch.css";

/** The strip above the page: something to drag the window by, and the running-call shortcut. */
function TopStrip() {
  const active = useStore(systemStore, (state) => state.active);
  return (
    <div className="hm-top" data-tauri-drag-region>
      {active ? (
        <div className="hm-callpill" role="status">
          <span className="live-dot" aria-hidden="true" />
          <span>Call running{active.company ? ` · ${active.company}` : ""}</span>
          <Button size="sm" variant="primary" onClick={() => void returnToCall()}>Return to call</Button>
        </div>
      ) : null}
    </div>
  );
}

function Page() {
  const route = useRoute();
  const key = route.page === "copilots" ? `copilots/${route.param ?? ""}` : route.page;
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={key}
        className="hm-page-host"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0 }}
        transition={{ duration: BASE, ease: EASE_OUT }}
      >
        {route.page === "library" ? <LibraryPage filter={route.param} />
          : route.page === "history" ? <HistoryPage selectedId={route.param} />
          : route.page === "settings" ? <SettingsPage section={route.param} />
          : <CopilotsPage selectedId={route.param} />}
      </motion.div>
    </AnimatePresence>
  );
}

/** Root of the workspace window (the normal Aside window). */
export function HomeApp() {
  useEffect(() => {
    void refreshSystem();
    // Coming back to the window (say, after adding a key in another app) re-checks the setup.
    const onFocus = () => void refreshSystem();
    window.addEventListener("focus", onFocus);
    const stop = watchSystem(() => {
      void announceCallEnded();
      void refreshSystem();
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      stop();
    };
  }, []);

  const preview = !isTauriRuntime() && !isDemoMode();
  return (
    <div className="hm-root">
      <Sidebar />
      <main className="hm-main">
        <TopStrip />
        {preview ? (
          <div className="hm-previewnote"><Callout tone="warn" title="Plain web preview">{NOT_TAURI_MESSAGE}</Callout></div>
        ) : null}
        <Page />
      </main>
      <CallSaved />
      <CommandPalette />
      <QuitGuard />
      <ToastHost />
    </div>
  );
}
