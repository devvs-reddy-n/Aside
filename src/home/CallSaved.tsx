import { useEffect, useState } from "react";
import type { PastSessionSummary } from "../bridge/types";
import { formatDuration } from "../core/format";
import { createStore, useStore } from "../core/store";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Icon } from "../kit/Icon";
import { loadHistory, historyStore } from "./data";
import { navigate } from "./router";

const endedStore = createStore<{ session: PastSessionSummary | null }>({ session: null });

/** Called when the live window reports the call ended: refresh History and show what was saved. */
export async function announceCallEnded(): Promise<void> {
  // The backend finishes writing the call just after the event; give it a moment.
  await new Promise((resolve) => setTimeout(resolve, 700));
  await loadHistory();
  const latest = historyStore.get().sessions[0] ?? null;
  endedStore.set({ session: latest });
}

export function CallSaved() {
  const session = useStore(endedStore, (state) => state.session);
  const [open, setOpen] = useState(false);
  useEffect(() => setOpen(session !== null), [session]);
  const close = () => {
    setOpen(false);
    endedStore.set({ session: null });
  };
  if (!session) return null;
  return (
    <Dialog
      open={open}
      onClose={close}
      title="Call ended"
      subtitle={session.company ? `${session.title} · ${session.company}` : session.title}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={close}>Close</Button>
          <Button
            variant="primary"
            onClick={() => {
              close();
              navigate("history", session.id);
            }}
          >
            Open in History
          </Button>
        </>
      }
    >
      <div className="hm-saved">
        <span className="hm-saved__check"><Icon name="check" size={22} /></span>
        <p>
          {session.segment_count > 0
            ? "Saved to History."
            : "Listed in History. No transcript was saved."}
        </p>
        <dl className="hm-stats">
          <div><dt>Length</dt><dd>{formatDuration(session.duration_ms)}</dd></div>
          <div><dt>Lines</dt><dd>{session.segment_count}</dd></div>
        </dl>
      </div>
    </Dialog>
  );
}
