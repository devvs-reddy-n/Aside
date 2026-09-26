import { useEffect, useMemo, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotInput, CopilotSummary, DocType, LlmProviderKind, StoredDocument } from "../../bridge/types";
import { formatBytes } from "../../core/format";
import { useStore } from "../../core/store";
import { Callout } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Select, TextArea, Toggle } from "../../kit/Form";
import { Icon, type IconName } from "../../kit/Icon";
import { toast } from "../../kit/toast";
import { ingestPaths, libraryStore, loadLibrary, pickDocumentFiles, saveCopilot } from "../data";
import { modelLabel } from "../labels";

const EMPTY: CopilotInput = {
  name: "", company: null, target_role: null, job_description: null, instructions: null,
  save_transcript: true, llm_provider: "open_ai", llm_model: "gpt-5.6-luna", resume_id: null, document_ids: [],
};

interface ModelOption { provider: LlmProviderKind; model: string }

export interface CopilotFormProps {
  /** null = creating a new one. */
  copilot: CopilotSummary | null;
  onDelete?: () => void;
  /** Leave the "new copilot" form without saving. */
  onCancelNew?: () => void;
  /** Called after a successful save. */
  onSaved: (saved: Copilot) => void;
  /** Asks the page to open the Start window for this copilot (after saving it if needed). */
  onStart?: (saved: Copilot | CopilotSummary) => void;
}

const same = (a: CopilotInput, b: CopilotInput) => JSON.stringify(a) === JSON.stringify(b);

export function CopilotForm({ copilot, onDelete, onCancelNew, onSaved, onStart }: CopilotFormProps) {
  const docs = useStore(libraryStore, (state) => state.docs);
  const libraryLoaded = useStore(libraryStore, (state) => state.loaded);
  const initial = useMemo<CopilotInput>(
    () => (copilot ? { ...EMPTY, ...copilot, company: copilot.company || copilot.name, resume_id: null, document_ids: [] } : EMPTY),
    [copilot],
  );
  const [input, setInput] = useState<CopilotInput>(initial);
  const [baseline, setBaseline] = useState<CopilotInput>(initial);
  const [sourcesReady, setSourcesReady] = useState(!copilot);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [saving, setSaving] = useState<"save" | "start" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nameError, setNameError] = useState<string | null>(null);
  const [importUrl, setImportUrl] = useState("");
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  /** Which long-text or file picker is open over the page. */
  const [sheet, setSheet] = useState<"job" | "guidance" | "files" | null>(null);

  const dirty = sourcesReady && !same(input, baseline);
  const set = <K extends keyof CopilotInput>(key: K, value: CopilotInput[K]) => setInput((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    if (!libraryLoaded) void loadLibrary();
  }, [libraryLoaded]);

  useEffect(() => {
    if (!copilot) return;
    let alive = true;
    void api.copilots.sources(copilot.id).then((result) => {
      if (!alive) return;
      if (result.ok) {
        const patch = {
          resume_id: result.value.find((d) => d.doc_type === "resume")?.id ?? null,
          document_ids: result.value.filter((d) => d.doc_type !== "resume").map((d) => d.id),
        };
        setInput((current) => ({ ...current, ...patch }));
        setBaseline((current) => ({ ...current, ...patch }));
      } else {
        setError(result.error);
      }
      setSourcesReady(true);
    });
    return () => {
      alive = false;
    };
  }, [copilot]);

  useEffect(() => {
    let alive = true;
    void (async () => {
      const providers = await api.model.providers();
      if (!providers.ok) return;
      const lists = await Promise.all(providers.value.map(async (provider) => {
        const result = await api.model.models(provider);
        return result.ok ? result.value.map((model) => ({ provider, model })) : [];
      }));
      if (alive) setModels(lists.flat());
    })();
    return () => {
      alive = false;
    };
  }, []);

  const resumes = docs.filter((doc) => doc.doc_type === "resume");
  const extras = docs.filter((doc) => doc.doc_type === "other");
  const modelKey = `${input.llm_provider}|${input.llm_model}`;
  const options = models.some((m) => `${m.provider}|${m.model}` === modelKey) || models.length === 0
    ? models
    : [{ provider: input.llm_provider, model: input.llm_model }, ...models];

  async function addFiles(docType: DocType) {
    const paths = await pickDocumentFiles();
    if (paths.length === 0) return;
    const report = await ingestPaths(paths, docType);
    for (const failure of report.failed) toast(failure.error, { tone: "bad" });
    if (report.added.length === 0) return;
    if (docType === "resume") set("resume_id", report.added[0].document_id);
    else setInput((current) => ({ ...current, document_ids: [...current.document_ids, ...report.added.map((d) => d.document_id)] }));
  }

  async function importJobPost() {
    setImportError(null);
    setImporting(true);
    const result = await api.documents.fetchJobPost(importUrl);
    setImporting(false);
    if (!result.ok) {
      setImportError(result.error);
      return;
    }
    set("job_description", result.value);
    setImportUrl("");
    toast("Imported.", { tone: "ok" });
  }

  async function save(start: boolean) {
    setError(null);
    const company = (input.company ?? "").trim();
    if (!company) {
      setNameError("Add a company.");
      return;
    }
    setNameError(null);
    setSaving(start ? "start" : "save");
    const result = await saveCopilot(copilot?.id ?? null, { ...input, name: company, company });
    setSaving(null);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setBaseline({ ...input, name: company, company });
    onSaved(result.value);
    if (start) onStart?.(result.value);
  }

  const isNew = copilot === null;
  const chosenFiles = extras.filter((doc) => input.document_ids.includes(doc.id));
  return (
    <div className="cf">
      <div className="cf-body">
        <header className="cf-head">
          <input
            className="cf-company"
            value={input.company ?? ""}
            placeholder="Company"
            aria-label="Company"
            aria-invalid={nameError ? true : undefined}
            onChange={(e) => { set("company", e.target.value || null); setNameError(null); }}
            autoFocus={isNew}
          />
          <input className="cf-role" value={input.target_role ?? ""} placeholder="Role" aria-label="Role" onChange={(e) => set("target_role", e.target.value || null)} />
          {nameError ? <p className="cf-error" role="alert">{nameError}</p> : null}
        </header>

        <Group title="The job">
          <Row icon="link" label="Job post link" hint={importError ?? undefined} bad={Boolean(importError)}>
            <input
              className="cf-inline"
              value={importUrl}
              placeholder="Paste a link to fill the description"
              aria-label="Job post link"
              onChange={(e) => { setImportUrl(e.target.value); setImportError(null); }}
              onKeyDown={(e) => { if (e.key === "Enter" && importUrl.trim()) void importJobPost(); }}
            />
            <Button size="sm" variant="secondary" busy={importing} disabled={!importUrl.trim()} onClick={() => void importJobPost()}>Import</Button>
          </Row>
          <Row icon="doc" label="Description" preview={input.job_description} empty="Not added">
            <Button size="sm" variant="secondary" onClick={() => setSheet("job")}>{input.job_description ? "Edit" : "Add"}</Button>
          </Row>
        </Group>

        <Group title="Your background">
          <Row icon="briefcase" label="Resume">
            <Select aria-label="Resume" value={input.resume_id ?? ""} disabled={!sourcesReady} onChange={(e) => set("resume_id", e.target.value || null)}>
              <option value="">None</option>
              {resumes.map((doc) => <option key={doc.id} value={doc.id}>{doc.file_name}</option>)}
            </Select>
            <Button size="sm" variant="quiet" icon="plus" aria-label="Add a resume file" title="Add a resume file" onClick={() => void addFiles("resume")} />
          </Row>
          <Row icon="note" label="Other files" preview={chosenFiles.map((doc) => doc.file_name).join(", ")} empty={extras.length === 0 ? "None saved" : "None chosen"}>
            <Button size="sm" variant="secondary" disabled={!sourcesReady} onClick={() => setSheet("files")}>Choose</Button>
          </Row>
        </Group>

        <Group title="Answers">
          <Row icon="edit" label="Guidance" preview={input.instructions} empty="Tone, focus, what to avoid">
            <Button size="sm" variant="secondary" onClick={() => setSheet("guidance")}>{input.instructions ? "Edit" : "Add"}</Button>
          </Row>
          <Row icon="sparkle" label="AI model">
            <Select aria-label="AI model" value={modelKey} onChange={(e) => { const [provider, ...rest] = e.target.value.split("|"); setInput((c) => ({ ...c, llm_provider: provider as LlmProviderKind, llm_model: rest.join("|") })); }}>
              {options.length === 0 ? <option value={modelKey}>{modelLabel(input.llm_provider, input.llm_model)}</option> : null}
              {options.map((option) => <option key={`${option.provider}|${option.model}`} value={`${option.provider}|${option.model}`}>{modelLabel(option.provider, option.model)}</option>)}
            </Select>
          </Row>
          <Row icon="clock" label="Save transcript">
            <Toggle checked={input.save_transcript} label="Save transcript" onChange={(next) => set("save_transcript", next)} />
          </Row>
        </Group>

        {error ? <Callout tone="bad" title="Couldn't save">{error}</Callout> : null}
      </div>

      <footer className="cf-bar">
        {!isNew && onDelete ? <Button variant="quiet" className="cf-delete" icon="trash" onClick={onDelete}>Delete</Button> : null}
        <span className="cf-bar__note" aria-live="polite">{dirty ? "Unsaved changes" : ""}</span>
        {isNew ? (
          <>
            <Button variant="ghost" onClick={onCancelNew}>Cancel</Button>
            <Button variant="primary" busy={saving === "save"} disabled={saving !== null} onClick={() => void save(false)}>Create</Button>
          </>
        ) : (
          <>
            <Button variant="secondary" busy={saving === "save"} disabled={saving !== null || !dirty || !sourcesReady} onClick={() => void save(false)}>Save</Button>
            <Button variant="primary" iconRight="arrow" busy={saving === "start"} disabled={saving !== null || !sourcesReady} onClick={() => (dirty ? void save(true) : onStart?.(copilot))}>Start session</Button>
          </>
        )}
      </footer>

      {sheet === "job" ? (
        <TextSheet title="Job description" value={input.job_description ?? ""} onClose={() => setSheet(null)} onDone={(text) => set("job_description", text || null)} />
      ) : null}
      {sheet === "guidance" ? (
        <TextSheet title="Guidance" placeholder="Tone, focus, what to lean on or avoid. Only Aside sees this." value={input.instructions ?? ""} onClose={() => setSheet(null)} onDone={(text) => set("instructions", text || null)} />
      ) : null}
      {sheet === "files" ? (
        <Dialog open onClose={() => setSheet(null)} size="sm" title="Other files"
          footer={<><Button variant="quiet" icon="plus" onClick={() => void addFiles("other")}>Add file</Button><span className="grow" /><Button variant="primary" onClick={() => setSheet(null)}>Done</Button></>}>
          <SourcePicker docs={extras} selected={input.document_ids} empty="None saved." onToggle={(id) => set("document_ids", input.document_ids.includes(id) ? input.document_ids.filter((x) => x !== id) : [...input.document_ids, id])} />
        </Dialog>
      ) : null}
    </div>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="cf-group" aria-label={title}>
      <h3>{title}</h3>
      <div className="cf-list">{children}</div>
    </section>
  );
}

/** One line: icon and name on the left, a one-line preview or its control, then its action. */
function Row({ icon, label, hint, bad, preview, empty, children }: {
  icon: IconName; label: string; hint?: string; bad?: boolean; preview?: string | null; empty?: string; children: ReactNode;
}) {
  const showsPreview = preview !== undefined;
  return (
    <div className="cf-row">
      <span className="cf-row__icon" aria-hidden="true"><Icon name={icon} size={15} /></span>
      <span className="cf-row__label">
        <b>{label}</b>
        {hint ? <small data-bad={bad || undefined}>{hint}</small> : null}
      </span>
      {showsPreview ? <span className="cf-row__preview" data-empty={!preview || undefined} title={preview ?? undefined}>{preview || empty}</span> : null}
      <span className="cf-row__control" data-fill={showsPreview ? undefined : ""}>{children}</span>
    </div>
  );
}

/** Long text gets a proper editor instead of a cramped box on the page. */
function TextSheet({ title, value, placeholder, onClose, onDone }: { title: string; value: string; placeholder?: string; onClose: () => void; onDone: (text: string) => void }) {
  const [draft, setDraft] = useState(value);
  return (
    <Dialog open onClose={onClose} size="lg" title={title}
      footer={<><Button variant="ghost" onClick={onClose}>Cancel</Button><Button variant="primary" onClick={() => { onDone(draft.trim()); onClose(); }}>Done</Button></>}>
      <TextArea className="cf-sheet-text" value={draft} placeholder={placeholder} aria-label={title} onChange={(e) => setDraft(e.target.value)} data-autofocus />
    </Dialog>
  );
}

function SourcePicker({ docs, selected, empty, onToggle }: { docs: StoredDocument[]; selected: string[]; empty: string; onToggle: (id: string) => void }) {
  if (docs.length === 0) return <p className="hm-muted">{empty}</p>;
  return (
    <ul className="hm-picker__list">
      {docs.map((doc) => (
        <li key={doc.id}>
          <label className="hm-pick">
            <input type="checkbox" checked={selected.includes(doc.id)} onChange={() => onToggle(doc.id)} />
            <Icon name="doc" size={16} />
            <span className="hm-pick__name">{doc.file_name}</span>
            <small>{formatBytes(doc.byte_size)}</small>
          </label>
        </li>
      ))}
    </ul>
  );
}
