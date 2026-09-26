import { useEffect } from "react";
import { Callout } from "../kit/Bits";
import { Button } from "../kit/Button";
import { ToastHost } from "../kit/toast";
import { useStore } from "../core/store";
import { isTauriRuntime, isDemoMode, NOT_TAURI_MESSAGE } from "../bridge/runtime";
import { CallSaved, announceCallEnded } from "./CallSaved";
import { Chrome } from "./Chrome";
import { CopilotsPage } from "./pages/CopilotsPage";
import { HistoryPage } from "./pages/HistoryPage";
import { LibraryPage } from "./pages/LibraryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { refreshSystem, returnToCall, systemStore, watchSystem } from "./data";
import { useRoute } from "./router";
import { Sidebar } from "./Sidebar";
import "./home.css";
import "./launch.css";

function ActiveCallBar() {
  const active = useStore(systemStore, (state) => state.active);
  if (!active) return null;
  return (
    <div className="hm-callbar" role="status">
      <span className="hm-callbar__dot" aria-hidden="true" />
      <span>A call is running{active.company ? ` with ${active.company}` : ""}.</span>
      <Button size="sm" variant="primary" onClick={() => void returnToCall()}>Return to call</Button>
    </div>
  );
}

function Page() {
  const route = useRoute();
  switch (route.page) {
    case "resumes": return <LibraryPage kind="resume" />;
    case "documents": return <LibraryPage kind="other" />;
    case "history": return <HistoryPage selectedId={route.param} />;
    case "settings": return <SettingsPage section={route.param} />;
    default: return <CopilotsPage selectedId={route.param} />;
  }
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
      <Chrome />
      <div className="hm-body">
        <Sidebar />
        <main className="hm-main">
          {preview ? (
            <div className="hm-previewnote"><Callout tone="warn" title="Plain web preview">{NOT_TAURI_MESSAGE}</Callout></div>
          ) : null}
          <ActiveCallBar />
          <Page />
        </main>
      </div>
      <CallSaved />
      <ToastHost />
    </div>
  );
}
