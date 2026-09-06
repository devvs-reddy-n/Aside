import { useEffect, useMemo, useState } from "react";
import { Button, Dialog, Icon } from "../../ui";
import type { PastSessionSummary, TranscriptTurn } from "../../platform/types";
import { downloadPdf, downloadText, recordFilename } from "./downloads";

export interface ConversationDialogProps {
  session: PastSessionSummary | null;
  onClose: () => void;
  getTranscript: (sessionId: string) => Promise<TranscriptTurn[] | null>;
}

function formatTranscript(turns: TranscriptTurn[], title: string): string {
  return [`Conversation: ${title}`, "", ...turns.map((turn) => `${turn.speaker === "you" ? "Me" : "Other"}: ${turn.text}`)].join("\n");
}

export function ConversationDialog({ session, onClose, getTranscript }: ConversationDialogProps) {
  const [turns, setTurns] = useState<TranscriptTurn[] | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!session) {
      setTurns(null);
      return;
    }
    setLoading(true);
    void getTranscript(session.id).then((value) => {
      setTurns(value ?? []);
      setLoading(false);
    });
  }, [session, getTranscript]);

  const exportText = useMemo(() => (session && turns ? formatTranscript(turns, session.title) : ""), [session, turns]);
  const exportBase = session ? recordFilename(session.title, "conversation") : "conversation";

  return (
    <Dialog
      open={session != null}
      onClose={onClose}
      title="Conversation"
      className="ss-view-dialog"
      footer={
        turns && turns.length > 0 ? (
          <div className="ss-export-actions">
            <Button variant="secondary" onClick={() => downloadText(`${exportBase}.txt`, exportText)}>
              <Icon name="download" size={14} /> Download text
            </Button>
            <Button variant="secondary" onClick={() => downloadPdf(`${exportBase}.pdf`, "Aside conversation", exportText.split("\n"))}>
              <Icon name="download" size={14} /> Download PDF
            </Button>
          </div>
        ) : undefined
      }
    >
      <p className="ss-dialog-subtitle">{session?.title}</p>
      {loading ? (
        <p className="u-field__hint">Loading conversation…</p>
      ) : turns && turns.length > 0 ? (
        <div className="ss-conversation" aria-label="Recorded conversation">
          {turns.map((turn, index) => (
            <div key={`${turn.start_ms}-${index}`} className={`ss-message ss-message--${turn.speaker}`}>
              <span className="ss-message__speaker">{turn.speaker === "you" ? "Me" : "Other"}</span>
              <p>{turn.text}</p>
            </div>
          ))}
        </div>
      ) : (
        <p className="u-field__hint">No recorded conversation is available for this call.</p>
      )}
    </Dialog>
  );
}
