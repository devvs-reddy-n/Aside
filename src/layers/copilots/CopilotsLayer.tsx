import { useEffect, useMemo, useState } from "react";
import { Dialog, Icon, Table, TableRow } from "../../ui";
import { invoke } from "../../platform/invoke";
import { useCopilots } from "../../state/copilots";
import { useDocuments } from "../../state/documents";
import type { CopilotInput, CopilotSummary, Session, StoredDocument } from "../../platform/types";

export interface CopilotsLayerProps { onStarted: (session: Session) => void; }

const EMPTY: CopilotInput = {
  name: "", company: null, target_role: null, job_description: null, instructions: null,
  save_transcript: true, llm_provider: "open_ai", llm_model: "gpt-5.6-luna",
  resume_id: null, document_ids: [],
};

function dateLabel(value: number) { return new Date(value).toLocaleDateString(undefined, { month: "short", day: "numeric" }); }

export function CopilotsLayer({ onStarted }: CopilotsLayerProps) {
  const store = useCopilots();
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<CopilotSummary | null | "new">(null);

  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase();
    return store.copilots.filter((copilot) => !term || [copilot.name, copilot.company, copilot.target_role].filter(Boolean).join(" ").toLowerCase().includes(term));
  }, [store.copilots, query]);

  async function start(copilot: CopilotSummary) {
    const session = await store.start(copilot.id);
    if (session) onStarted(session);
  }

  return (
    <div className="cp-page">
      <header className="cp-hero">
        <div>
          <p className="cp-kicker">Your call setups</p>
          <h1>Copilots</h1>
        </div>
        <button className="cp-primary" type="button" onClick={() => setEditing("new")}><Icon name="plus" /> New Copilot</button>
      </header>
      <div className="cp-toolbar">
        <label className="cp-search"><Icon name="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Find a Copilot" /></label>
        <span className="cp-count">{store.copilots.length} saved</span>
      </div>
      {store.error ? <p className="sc-error-line">{store.error}</p> : null}
      {store.loading ? <div className="cp-empty"><p>Loading your Copilots…</p></div> : filtered.length === 0 ? (
        <div className="cp-empty">
          <span className="cp-empty__mark"><Icon name="spark" size={22} /></span>
          <h2>{store.copilots.length ? "No Copilot matches that search" : "Build your first call setup"}</h2>
          <p>{store.copilots.length ? "Try another search." : "Add a role, resume, and instructions to get started."}</p>
          {!store.copilots.length ? <button className="cp-primary" type="button" onClick={() => setEditing("new")}>Create Copilot</button> : null}
        </div>
      ) : (
        <Table className="cp-table" ariaLabel="Copilots">
          <TableRow head className="cp-table__row cp-table__row--head">
            <span role="columnheader" className="cp-table__name-head">
              <span className="cp-card__initial cp-card__initial--sm u-invisible-spacer" aria-hidden="true" />
              Name
            </span>
            <span role="columnheader">Sources</span>
            <span role="columnheader">Model</span>
            <span role="columnheader">Updated</span>
            <span role="columnheader" aria-hidden="true" />
          </TableRow>
          {filtered.map((copilot) => <CopilotTableRow key={copilot.id} copilot={copilot} onOpen={() => setEditing(copilot)} onStart={() => void start(copilot)} />)}
        </Table>
      )}
      {editing ? <CopilotPanel copilot={editing === "new" ? null : editing} onClose={() => setEditing(null)} onStarted={onStarted} onSave={store.save} onStart={store.start} onDelete={store.remove} /> : null}
    </div>
  );
}

function CopilotTableRow({ copilot, onOpen, onStart }: { copilot: CopilotSummary; onOpen: () => void; onStart: () => void }) {
  return (
    <TableRow className="cp-table__row">
      <button type="button" className="cp-table__name" onClick={onOpen}>
        <span className="cp-card__initial cp-card__initial--sm">{copilot.name.slice(0, 1).toUpperCase()}</span>
        <span className="cp-table__name-copy">
          <strong>{copilot.name}</strong>
          <span className="cp-table__subtitle">{[copilot.company, copilot.target_role].filter(Boolean).join(" · ") || "Call setup"}</span>
        </span>
      </button>
      <span>{copilot.document_count} source{copilot.document_count === 1 ? "" : "s"}</span>
      <span>{copilot.llm_model}</span>
      <span>{dateLabel(copilot.updated_at_ms)}</span>
      <span className="cp-table__actions">
        <button type="button" className="cp-start" onClick={onStart}>
          Start <Icon name="arrowRight" size={13} />
        </button>
      </span>
    </TableRow>
  );
}

function CopilotPanel({ copilot, onClose, onSave, onStart, onStarted, onDelete }: {
  copilot: CopilotSummary | null; onClose: () => void; onSave: (id: string | null, input: CopilotInput) => Promise<unknown>;
  onStart: (id: string) => Promise<Session | null>; onStarted: (session: Session) => void; onDelete: (id: string) => Promise<boolean>;
}) {
  const documents = useDocuments(null);
  const [input, setInput] = useState<CopilotInput>(() => copilot ? { ...EMPTY, ...copilot, resume_id: null, document_ids: [] } : EMPTY);
  const [loadingSources, setLoadingSources] = useState(Boolean(copilot));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>(["gpt-5.6-luna"]);

  useEffect(() => {
    if (!copilot) return;
    void (async () => {
      const sources = await invoke<StoredDocument[]>("get_copilot_documents", { copilotId: copilot.id });
      if (sources.ok) setInput((current) => ({ ...current, resume_id: sources.value.find((document) => document.doc_type === "resume")?.id ?? null, document_ids: sources.value.filter((document) => document.doc_type !== "resume").map((document) => document.id) }));
      else setError(sources.error);
      setLoadingSources(false);
    })();
  }, [copilot]);
  useEffect(() => { void (async () => {
    const result = await invoke<string[]>("list_llm_models", { provider: input.llm_provider });
    if (result.ok && result.value.length) { setModels(result.value); setInput((value) => ({ ...value, llm_model: result.value.includes(value.llm_model) ? value.llm_model : result.value[0] })); }
  })(); }, [input.llm_provider]);
  const set = <K extends keyof CopilotInput>(key: K, value: CopilotInput[K]) => setInput((current) => ({ ...current, [key]: value }));
  const resumes = documents.documents.filter((document) => document.doc_type === "resume");
  const extras = documents.documents.filter((document) => document.doc_type === "other");
  async function upload(kind: "resume" | "other") { const uploaded = await documents.uploadFile(kind); if (uploaded) { if (kind === "resume") set("resume_id", uploaded.document_id); else set("document_ids", [...input.document_ids, uploaded.document_id]); } }
  async function saveAndMaybeStart(startAfterSave: boolean) {
    setSaving(true); setError(null);
    const saved = await onSave(copilot?.id ?? null, input) as { id?: string } | null;
    setSaving(false);
    if (!saved || !saved.id) { setError("Aside could not save this Copilot. Check the fields and try again."); return; }
    if (startAfterSave) { const session = await onStart(saved.id); if (session) onStarted(session); } else onClose();
  }
  return <Dialog
    open
    onClose={onClose}
    className="cp-editor-dialog"
    title={<span className="cp-editor-dialog__heading"><span className="cp-editor-dialog__title-copy"><span className="cp-editor-dialog__kicker">Copilot setup</span><span>{copilot ? `Edit ${copilot.name}` : "Create a Copilot"}</span></span><button type="button" className="cp-icon-button" onClick={onClose} aria-label="Close"><Icon name="close" /></button></span>}
    footer={<><button type="button" className="cp-quiet" onClick={() => copilot && void onDelete(copilot.id).then(onClose)}>{copilot ? "Delete" : "Cancel"}</button><span /><button type="button" className="cp-secondary" disabled={saving} onClick={() => void saveAndMaybeStart(false)}>{saving ? "Saving…" : "Save setup"}</button><button type="button" className="cp-primary" disabled={saving} onClick={() => void saveAndMaybeStart(true)}>{saving ? "Saving…" : "Save & start"}<Icon name="arrowRight" /></button></>}
  >
      <div className="cp-editor-dialog__scroll" aria-label={copilot ? `Edit ${copilot.name}` : "New Copilot"}>
        <PanelSection title="Call details">
          <div className="cp-details-grid"><label>Copilot name<input value={input.name} onChange={(event) => set("name", event.target.value)} placeholder="e.g. Nimbus product interview" /></label><label>Target role<input value={input.target_role ?? ""} onChange={(event) => set("target_role", event.target.value || null)} placeholder="Role" /></label><label>Company<input value={input.company ?? ""} onChange={(event) => set("company", event.target.value || null)} placeholder="Company" /></label><label className="cp-details-grid__description">Job description<textarea rows={4} value={input.job_description ?? ""} onChange={(event) => set("job_description", event.target.value || null)} placeholder="Role and responsibilities" /></label></div>
        </PanelSection>
        <PanelSection title="Sources">
          {loadingSources ? <p className="cp-muted">Loading saved sources…</p> : <><SourceSelect title="Resume" items={resumes} selected={input.resume_id ? [input.resume_id] : []} single onToggle={(id) => set("resume_id", input.resume_id === id ? null : id)} onUpload={() => void upload("resume")} /><SourceSelect title="Supporting material" items={extras} selected={input.document_ids} onToggle={(id) => set("document_ids", input.document_ids.includes(id) ? input.document_ids.filter((current) => current !== id) : [...input.document_ids, id])} onUpload={() => void upload("other")} /></>}
        </PanelSection>
        <div className="cp-preferences-grid">
          <PanelSection title="Response preferences"><label>Private guidance<textarea rows={8} value={input.instructions ?? ""} onChange={(event) => set("instructions", event.target.value || null)} placeholder="Tone, focus, and anything else you want Aside to know" /></label></PanelSection>
          <div className="cp-preferences-grid__right">
            <PanelSection title="Save call record">
              <label className="cp-record-toggle"><span><strong>Save transcript</strong><small>Keep this call in your private history.</small></span><input type="checkbox" checked={input.save_transcript} onChange={(event) => set("save_transcript", event.target.checked)} /></label>
              {input.save_transcript ? <div className="cp-record-details"><span>Saved after the call</span><div><span><Icon name="message" size={13} /> Transcript</span><span><Icon name="note" size={13} /> Context notes</span><span><Icon name="spark" size={13} /> AI summary</span></div></div> : null}
            </PanelSection>
            <PanelSection title="AI model" className="cp-section--model"><label>Model<select value={input.llm_model} onChange={(event) => set("llm_model", event.target.value)}>{models.map((model) => <option key={model}>{model}</option>)}</select></label></PanelSection>
          </div>
        </div>
        {error || documents.error ? <p className="sc-error-line">{error ?? documents.error}</p> : null}
      </div>
  </Dialog>;
}

function PanelSection({ title, children, className = "" }: { title: string; children: React.ReactNode; className?: string }) { return <section className={["cp-section", className].filter(Boolean).join(" ")}><div className="cp-section__heading"><h3>{title}</h3></div><div className="cp-section__content">{children}</div></section>; }
function SourceSelect({ title, items, selected, single = false, onToggle, onUpload }: { title: string; items: StoredDocument[]; selected: string[]; single?: boolean; onToggle: (id: string) => void; onUpload: () => void }) { return <div className="cp-source"><div><strong>{title}</strong><button type="button" onClick={onUpload}><Icon name="plus" size={12} /> Upload</button></div>{items.length ? <ul>{items.map((item) => <li key={item.id}><label><input type={single ? "radio" : "checkbox"} checked={selected.includes(item.id)} onChange={() => onToggle(item.id)} /> <Icon name="document" size={13} /> <span>{item.file_name}</span></label></li>)}</ul> : <p>No saved {title.toLowerCase()} yet.</p>}</div>; }
