import { AnimatePresence, motion } from "motion/react";
import { useEffect } from "react";
import { Callout } from "../kit/Bits";
import { Button } from "../kit/Button";
import { SPRING } from "../kit/motion";
import { ToastHost } from "../kit/toast";
import { useStore } from "../core/store";
import { isTauriRuntime, isDemoMode, NOT_TAURI_MESSAGE } from "../bridge/runtime";
import { CallSaved, announceCallEnded } from "./CallSaved";
import { QuitGuard } from "./Chrome";
import { CopilotsPage } from "./pages/CopilotsPage";
import { HistoryPage } from "./pages/HistoryPage";
import { LibraryPage } from "./pages/LibraryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { refreshSystem, returnToCall, systemStore, watchSystem } from "./data";
import { useRoute } from "./router";
import { Sidebar } from "./Sidebar";
import "./home.css";
import "./launch.css";

/** The strip above the page: room to drag the window, and the running call on the right. */
function TopStrip() {
  const active = useStore(systemStore, (state) => state.active);
  return (
    <div className="hm-top" data-tauri-drag-region>
      <div className="hm-top__end" data-tauri-drag-region>
        <AnimatePresence initial={false}>
          {active ? (
            <motion.div
              className="hm-callpill"
              role="status"
              initial={{ opacity: 0, scale: 0.9, x: 12 }}
              animate={{ opacity: 1, scale: 1, x: 0 }}
              exit={{ opacity: 0, scale: 0.9, x: 12 }}
              transition={SPRING}
            >
              <span className="live-dot" aria-hidden="true" />
              <span className="hm-callpill__text">{active.company ? active.company : "Call running"}</span>
              <Button size="sm" variant="primary" onClick={() => void returnToCall()}>Return to call</Button>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </div>
  );
}

function Page() {
  const route = useRoute();
  // Switching between copilots stays inside one page (the sidebar highlight glides; no page fade).
  const key = route.page;
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={key}
        className="hm-page-host"
        initial={{ opacity: 0, y: 10, scale: 0.995 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: -4, transition: { duration: 0.1 } }}
        transition={SPRING}
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
      <QuitGuard />
      <ToastHost />
    </div>
  );
}
