import { invoke } from "../../platform/invoke";
import { Icon, TableRow } from "../../ui";
import type { PastSessionSummary } from "../../platform/types";

export interface SessionRowProps {
  session: PastSessionSummary;
  /** True only when this row is both `status === "active"` *and* is the
   *  one session the app currently tracks as live — Aside has no concept
   *  of resuming any other past session's call. */
  isLiveActive: boolean;
  onConversation: () => void;
  onContext: () => void;
  onSummary: () => void;
  onDelete: () => void;
}

function formatDuration(ms: number | null): string {
  if (ms == null) return "Not available";
  const totalSeconds = Math.round(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds.toString().padStart(2, "0")}s`;
}

function formatDate(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

async function join(): Promise<void> {
  await invoke("show_main_window");
  await invoke("open_overlay");
}

export function SessionRow({ session, isLiveActive, onConversation, onContext, onSummary, onDelete }: SessionRowProps) {
  return (
    <TableRow className="ss-row">
      <span role="cell" className="ss-cell-title">
        <span className="ss-title">{session.title}</span>
        {session.company ? <span className="ss-meta">{session.company}</span> : null}
      </span>
      <span role="cell">{formatDuration(session.duration_ms)}</span>
      <span role="cell">{formatDate(session.started_at_ms)}</span>
      <span role="cell">
        <span className={`ss-state ss-state--${session.status}`}>
          <span className="u-status-orb" aria-hidden="true" />
          {session.status === "active" ? "Active" : session.status === "ended" ? "Ended" : "Draft"}
        </span>
      </span>
      <span role="cell" className="ss-col-action">
        {session.status === "active" ? (
          <button type="button" className="ss-action" disabled={!isLiveActive} onClick={() => void join()}>
            Join <Icon name="arrowRight" size={13} />
          </button>
        ) : (
          <>
            <button type="button" className="ss-action ss-action--icon" onClick={onConversation} aria-label="View conversation" title="Conversation">
              <Icon name="message" size={15} />
            </button>
            <button type="button" className="ss-action ss-action--icon" onClick={onContext} aria-label="View context notes" title="Context notes">
              <Icon name="note" size={15} />
            </button>
            <button type="button" className="ss-action ss-action--icon" onClick={onSummary} aria-label="Summarize conversation" title="AI summary">
              <Icon name="spark" size={15} />
            </button>
          </>
        )}
        <button type="button" className="ss-action ss-action--danger ss-action--icon" aria-label="Delete session" title="Delete" onClick={onDelete}>
          <Icon name="trash" size={13} />
        </button>
      </span>
    </TableRow>
  );
}
