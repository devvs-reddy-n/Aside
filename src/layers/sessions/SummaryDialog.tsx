import { useEffect, useState } from "react";
import { Button, Dialog, Icon } from "../../ui";
import type { PastSessionSummary, SessionSummary } from "../../platform/types";

export interface SummaryDialogProps {
  session: PastSessionSummary | null;
  onClose: () => void;
  getSummary: (sessionId: string) => Promise<SessionSummary | null>;
}

export function SummaryDialog({ session, onClose, getSummary }: SummaryDialogProps) {
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSummary(null);
    setError(null);
    setLoading(false);
  }, [session]);

  const generate = async () => {
    if (!session) return;
    setLoading(true);
    setError(null);
    const value = await getSummary(session.id);
    if (value) setSummary(value);
    else setError("The summary could not be created. Check your AI settings and try again.");
    setLoading(false);
  };

  return (
    <Dialog
      open={session != null}
      onClose={onClose}
      title="AI summary"
      className="ss-view-dialog"
      footer={
        <Button variant="primary" onClick={() => void generate()} disabled={loading}>
          <Icon name="spark" size={14} /> {loading ? "Summarizing…" : summary ? "Refresh summary" : "Generate summary"}
        </Button>
      }
    >
      <p className="ss-dialog-subtitle">{session?.title}</p>
      {summary ? (
        <div className="ss-summary">
          <section>
            <h3>Conversation overview</h3>
            <p>{summary.paragraph}</p>
          </section>
          <section>
            <h3>Important notes</h3>
            <ul>
              {summary.highlights.map((item, index) => (
                <li key={`${index}-${item}`}>{item}</li>
              ))}
            </ul>
          </section>
        </div>
      ) : error ? (
        <p className="sc-error-line">{error}</p>
      ) : (
        <p className="u-field__hint">Create a factual overview and a short list of the important notes from the recorded conversation.</p>
      )}
    </Dialog>
  );
}
