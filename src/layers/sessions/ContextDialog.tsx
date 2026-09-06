import { useEffect, useMemo, useState } from "react";
import { Button, Dialog, Icon } from "../../ui";
import type { PastSessionSummary } from "../../platform/types";
import { downloadPdf, downloadText, recordFilename } from "./downloads";

export interface ContextDialogProps {
  session: PastSessionSummary | null;
  onClose: () => void;
  getContext: (sessionId: string) => Promise<string[] | null>;
}

function formatContext(context: string[], title: string): string {
  return [`Context notes: ${title}`, "", ...context.map((line) => `- ${line}`)].join("\n");
}

export function ContextDialog({ session, onClose, getContext }: ContextDialogProps) {
  const [context, setContext] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!session) {
      setContext(null);
      return;
    }
    setLoading(true);
    void getContext(session.id).then((value) => {
      setContext(value ?? []);
      setLoading(false);
    });
  }, [session, getContext]);

  const exportText = useMemo(() => (session && context ? formatContext(context, session.title) : ""), [session, context]);
  const exportBase = session ? recordFilename(session.title, "context") : "context";

  return (
    <Dialog
      open={session != null}
      onClose={onClose}
      title="Context notes"
      className="ss-view-dialog"
      footer={
        context && context.length > 0 ? (
          <div className="ss-export-actions">
            <Button variant="secondary" onClick={() => downloadText(`${exportBase}.txt`, exportText)}>
              <Icon name="download" size={14} /> Download text
            </Button>
            <Button variant="secondary" onClick={() => downloadPdf(`${exportBase}.pdf`, "Aside context notes", exportText.split("\n"))}>
              <Icon name="download" size={14} /> Download PDF
            </Button>
          </div>
        ) : undefined
      }
    >
      <p className="ss-dialog-subtitle">{session?.title}</p>
      {loading ? (
        <p className="u-field__hint">Loading context notes…</p>
      ) : context && context.length > 0 ? (
        <ul className="ss-context-list">
          {context.map((line, index) => (
            <li key={`${index}-${line}`}>{line}</li>
          ))}
        </ul>
      ) : (
        <p className="u-field__hint">No background context notes were saved for this call.</p>
      )}
    </Dialog>
  );
}
