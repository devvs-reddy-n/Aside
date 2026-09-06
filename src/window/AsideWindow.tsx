import { useState } from "react";
import { WindowChrome } from "./WindowChrome";
import { SettingsSheet } from "./SettingsSheet";
import { Sidebar, type NavDestination } from "./Sidebar";
import { ConnectStep } from "../layers/create/ConnectStep";
import { SessionsLayer } from "../layers/sessions/SessionsLayer";
import { ResumeLayer } from "../layers/resume/ResumeLayer";
import { DocumentsLayer } from "../layers/documents/DocumentsLayer";
import { useSession } from "../state/session";
import { CopilotsLayer } from "../layers/copilots/CopilotsLayer";
import { Dialog } from "../ui/Dialog";
import type { Session } from "../platform/types";

/**
 * Aside's one standalone product window (`tauri.conf.json`'s `main`
 * window). "Silk" neomorphic shell (DESIGN.md §3a): a nav sidebar plus
 * whichever destination is active. Replaces the old bare `WindowChrome`-only
 * layout — Settings stays a floating L4 sheet (a nav row opens it), and
 * Create/Connect keep their exact existing behaviour (`useCreateForm().step`
 * still drives the form-vs-connect switch) as one of the sidebar's
 * destinations rather than the window's only content. Every field, every
 * backend call, lives exactly where it already did in `layers/create/`,
 * `layers/sessions/`, `layers/resume/`, `layers/documents/`, and
 * `layers/settings/` — this file only decides what's on screen. There is no
 * standalone Context destination any more (2026-08-30) — that screen's
 * "conversation so far" list is now shown inside a session's own transcript
 * dialog in Call history instead (`layers/sessions/TranscriptDialog.tsx`),
 * since it's data *about* a specific session, not a screen of its own.
 */
export function AsideWindow() {
  const session = useSession();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [destination, setDestination] = useState<NavDestination>("copilots");
  const [startingCall, setStartingCall] = useState<Session | null>(null);

  const activeSessionId = session.active?.id ?? null;

  function renderDestination() {
    if (destination === "copilots") return <CopilotsLayer onStarted={setStartingCall} />;
    if (destination === "sessions") return <SessionsLayer onCreateNew={() => setDestination("copilots")} />;
    if (destination === "resume") return <ResumeLayer sessionId={activeSessionId} />;
    if (destination === "documents") return <DocumentsLayer sessionId={activeSessionId} />;
    return <CopilotsLayer onStarted={setStartingCall} />;
  }

  return (
    <div className="aw-root">
      <WindowChrome />
      <div className="aw-body">
        <Sidebar active={destination} onNavigate={setDestination} onOpenSettings={() => setSettingsOpen(true)} />
        <div className="aw-scroll">{renderDestination()}</div>
      </div>
      <SettingsSheet open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <Dialog open={!!startingCall} onClose={() => setStartingCall(null)} title="Connect call session" className="sc-connect-dialog">
        {startingCall ? <ConnectStep onBack={() => setStartingCall(null)} session={startingCall} /> : null}
      </Dialog>
    </div>
  );
}
