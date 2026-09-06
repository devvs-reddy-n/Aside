import { useMemo, useState } from "react";
import { usePastSessions } from "../../state/pastSessions";
import { useSession } from "../../state/session";
import { PillBar, Pill, Button, EmptyState, Icon, Table, TableRow } from "../../ui";
import { SessionRow } from "./SessionRow";
import { ConversationDialog } from "./ConversationDialog";
import { ContextDialog } from "./ContextDialog";
import { SummaryDialog } from "./SummaryDialog";
import { DeleteSessionDialog } from "./DeleteSessionDialog";
import type { PastSessionSummary } from "../../platform/types";

export interface SessionsLayerProps {
  onCreateNew: () => void;
}

type Filter = "all" | "active" | "ended";

/**
 * Call Sessions — real session history, rebuilt on top of the backend
 * that survived the earlier removal of this screen (`list_past_sessions`,
 * `get_session_transcript`, `search_transcripts`, `delete_session`; see
 * `docs/decisions.md`). Matches the owner's own Stitch mockup: filter
 * pills, a search box, and a table of Company + Role / Mode / Duration /
 * Created / Save Transcript / State / Action.
 */
export function SessionsLayer({ onCreateNew }: SessionsLayerProps) {
  const past = usePastSessions();
  const activeSession = useSession();
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [viewingConversation, setViewingConversation] = useState<PastSessionSummary | null>(null);
  const [viewingContext, setViewingContext] = useState<PastSessionSummary | null>(null);
  const [viewingSummary, setViewingSummary] = useState<PastSessionSummary | null>(null);
  const [deleting, setDeleting] = useState<PastSessionSummary | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return past.sessions.filter((s) => {
      if (filter !== "all" && s.status !== filter) return false;
      if (q.length === 0) return true;
      return s.title.toLowerCase().includes(q) || (s.company ?? "").toLowerCase().includes(q);
    });
  }, [past.sessions, filter, query]);

  return (
    <div className="ss-page">
      <header className="ss-page__header">
        <h1 className="ss-page__title">Call history</h1>
        <Button variant="primary" onClick={onCreateNew}>
          <Icon name="plus" />
          <span>New Copilot</span>
        </Button>
      </header>

      <div className="ss-toolbar">
        <PillBar>
          <Pill active={filter === "all"} onClick={() => setFilter("all")}>
            All
          </Pill>
          <Pill active={filter === "active"} onClick={() => setFilter("active")}>
            Active
          </Pill>
          <Pill active={filter === "ended"} onClick={() => setFilter("ended")}>
            Ended
          </Pill>
        </PillBar>
        <div className="ss-search">
          <Icon name="search" />
          <input
            type="search"
            placeholder="Search by title or company"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
      </div>

      {past.error ? <p className="sc-error-line">{past.error}</p> : null}

      {!past.loading && filtered.length === 0 ? (
        <EmptyState
          heading={past.sessions.length === 0 ? "No sessions yet" : "No sessions match"}
          description={
            past.sessions.length === 0
              ? "Start a call from a Copilot. It will appear here when it ends."
              : "Try a different search or filter."
          }
        />
      ) : (
        <Table className="ss-table" ariaLabel="Call sessions">
          <TableRow head className="ss-row ss-row--head">
            <span role="columnheader">Company + Role</span>
            <span role="columnheader">Duration</span>
            <span role="columnheader">Created</span>
            <span role="columnheader">State</span>
            <span role="columnheader" className="ss-col-action">
              Action
            </span>
          </TableRow>
          {filtered.map((s) => (
            <SessionRow
              key={s.id}
              session={s}
              isLiveActive={s.status === "active" && activeSession.active?.id === s.id}
              onConversation={() => setViewingConversation(s)}
              onContext={() => setViewingContext(s)}
              onSummary={() => setViewingSummary(s)}
              onDelete={() => setDeleting(s)}
            />
          ))}
        </Table>
      )}

      <ConversationDialog
        session={viewingConversation}
        onClose={() => setViewingConversation(null)}
        getTranscript={past.getTranscript}
      />
      <ContextDialog
        session={viewingContext}
        onClose={() => setViewingContext(null)}
        getContext={past.getContext}
      />
      <SummaryDialog session={viewingSummary} onClose={() => setViewingSummary(null)} getSummary={past.getSummary} />
      <DeleteSessionDialog
        session={deleting}
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          await past.remove(deleting.id);
          setDeleting(null);
        }}
      />
    </div>
  );
}
