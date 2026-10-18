import { useEffect, useState, type ReactNode } from "react";
import * as api from "../bridge/api";
import { Button } from "../kit/Button";
import { Sheet } from "../kit/Sheet";
import { SetupEditor, SetupSection } from "./SetupSection";
import type { IconName } from "../kit/Icon";

type Kind = "job_description" | "guidance";
// Reuse pending requests across React remounts; bounded in-memory results for fast navigation.
const requests = new Map<string, Promise<api.Result<string[]>>>();
function summarize(id: string | null, kind: Kind, text: string) {
  const key = JSON.stringify([id, kind, text]);
  const existing = requests.get(key);
  if (existing) return existing;
  const pending = api.copilots.summarize(id, kind, text);
  requests.set(key, pending);
  void pending.then((result) => {
    if (!result.ok) requests.delete(key);
    if (requests.size > 30) requests.delete(requests.keys().next().value!);
  });
  return pending;
}

export function SetupSummary({ name, title, icon, helper, action, copilotId, kind, value, placeholder, onChange, children, hidden = false }: {
  name: string; title: string; icon: IconName; helper?: ReactNode; action?: ReactNode;
  copilotId: string | null; kind: Kind; value: string | null; placeholder: string;
  onChange: (value: string | null) => void; children?: ReactNode; hidden?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [retry, setRetry] = useState(0);
  const [result, setResult] = useState<{ source: string; bullets?: string[]; error?: string }>();
  const text = value?.trim() ?? "";
  useEffect(() => {
    if (!text || hidden) return;
    let current = true;
    void summarize(copilotId, kind, text).then((response) => {
      if (!current) return;
      setResult(response.ok ? { source: text, bullets: response.value } : { source: text, error: response.error });
    });
    return () => { current = false; };
  }, [copilotId, kind, text, retry, hidden]);
  const summary = result?.source === text ? result : undefined;
  const edit = () => { setDraft(value ?? ""); setEditing(true); };
  return <SetupSection name={name} title={title} icon={icon} helper={helper}
    action={<><span className="setup-summary__caption">AI overview</span>{action}<Button size="sm" variant="ghost" icon="edit" aria-label={`Edit ${title.toLowerCase()}`} onClick={edit}>{text ? "Edit" : "Add"}</Button></>}>
    {children}
    {!hidden ? <div className="setup-summary" aria-label={`${title} AI summary`} aria-busy={Boolean(text && !summary)}>
      {!text ? <button type="button" className="setup-summary__empty" onClick={edit}>{placeholder}</button>
        : summary?.bullets ? <ul>{summary.bullets.map((point, index) => <li key={index}>{point}</li>)}</ul>
        : summary?.error ? <div className="setup-summary__message" role="status"><span>Couldn’t summarize this text.</span><small>{/key|model|connect/i.test(summary.error) ? "Check your AI connection in Settings, then try again." : /long/i.test(summary.error) ? "Shorten the original text in Edit, then try again." : "Your original text is safe. You can edit it or try the summary again."}</small><Button size="sm" variant="secondary" icon="refresh" onClick={() => { setResult(undefined); setRetry((v) => v + 1); }}>Try again</Button></div>
        : <div className="setup-summary__loading" role="status"><span className="sr-only">Summarizing into five points…</span>{[0,1,2,3,4].map((i) => <span key={i} className="setup-summary__skeleton" aria-hidden="true" />)}</div>}
    </div> : null}
    {editing ? <Sheet open title={`Edit ${title.toLowerCase()}`} meta="The original text is used during your session." onClose={() => setEditing(false)}
      footer={<><Button variant="ghost" onClick={() => setEditing(false)}>Cancel</Button><Button variant="primary" onClick={() => { onChange(draft.trim() || null); setEditing(false); }}>Save changes</Button></>}>
      <SetupEditor className="setup-summary__editor" autoFocus aria-label={`Full ${title.toLowerCase()}`} value={draft} placeholder={placeholder} onChange={(e) => setDraft(e.target.value)} />
    </Sheet> : null}
  </SetupSection>;
}
