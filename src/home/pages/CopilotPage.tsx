import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotInput, CopilotSummary, DocType, LlmProviderKind, StoredDocument } from "../../bridge/types";
import { formatBytes, formatDate } from "../../core/format";
import { useStore } from "../../core/store";
import { displayFileName, fileKind, isWebUrl, wordCount } from "../../core/text";
import { Button, IconButton } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Select, Toggle } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { Menu } from "../../kit/Menu";
import { EASE_OUT, FAST } from "../../kit/motion";
import { Popover } from "../../kit/Popover";
import { RichText } from "../../kit/RichText";
import { Sheet } from "../../kit/Sheet";
import { Skeleton } from "../../kit/Skeleton";
import { TextEditor } from "../../kit/TextEditor";
import { toast } from "../../kit/toast";
import { useAutoHeight } from "../../kit/useAutoHeight";
import { useAutosave } from "../../kit/useAutosave";
import type { SaveState } from "../../core/autosave";
import { onMessage } from "../bus";
import { ingestPaths, libraryStore, loadLibrary, pickDocumentFiles, saveCopilot, systemStore } from "../data";
import { useFileDrop } from "../fileDrop";
import { modelLabel } from "../labels";
import { hrefFor } from "../router";
import { DocumentViewer } from "./DocumentViewer";

interface ModelOption { provider: LlmProviderKind; model: string }

export interface CopilotPageProps {
  /** null = a new, not yet saved copilot. */
  copilot: CopilotSummary | null;
  /** A new copilot was created (first save). */
  onCreated: (saved: Copilot) => void;
  onDelete: () => void;
  onDuplicate: (copy: CopilotInput) => void;
  onStart: (copilot: CopilotSummary | Copilot) => void;
  /** Pre-fill the job link (from "Start from job link"). */
  startWithLink?: boolean;
}

const same = (a: CopilotInput, b: CopilotInput) => JSON.stringify(a) === JSON.stringify(b);

/* ------------------------------ small pieces ------------------------------ */

/** Title text you can click into and edit; long names wrap instead of being cut. Enter or leaving saves; Esc puts it back. */
function InlineTitle({ value, placeholder, className, onCommit, autoFocus, invalid }: {
  value: string; placeholder: string; className: string; onCommit: (next: string) => void; autoFocus?: boolean; invalid?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  const box = useAutoHeight(draft || placeholder);
  useEffect(() => setDraft(value), [value]);
  return (
    <textarea
      ref={box}
      rows={1}
      className={className}
      value={draft}
      placeholder={placeholder}
      aria-label={placeholder}
      aria-invalid={invalid || undefined}
      autoFocus={autoFocus}
      spellCheck={false}
      onChange={(event) => setDraft(event.target.value.replace(/\n/g, " "))}
      onBlur={() => { if (draft.trim() !== value.trim()) onCommit(draft.trim()); }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        }
        if (event.key === "Escape") {
          event.preventDefault();
          const el = event.currentTarget;
          setDraft(value);
          window.setTimeout(() => el.blur());
        }
      }}
    />
  );
}

function SaveIndicator({ state }: { state: SaveState }) {
  const text = state === "saving" || state === "pending" ? "Saving…" : state === "saved" ? "Saved" : state === "error" ? "Not saved" : "";
  return (
    <span className="save-state" data-state={state} aria-live="polite">
      <AnimatePresence mode="wait" initial={false}>
        {text ? (
          <motion.span key={text} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }}>
            {state === "saved" ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <motion.path d="M5 12.5l4.5 4.5L19 7.5" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.24, ease: EASE_OUT }} />
              </svg>
            ) : state === "error" ? <Icon name="alertCircle" size={14} /> : null}
            {text}
          </motion.span>
        ) : null}
      </AnimatePresence>
    </span>
  );
}

function Tile({ name, title, helper, children, action }: { name: string; title: string; helper?: ReactNode; children: ReactNode; action?: ReactNode }) {
  return (
    <section className={`cp-tile cp-tile--${name}`} aria-label={title}>
      <header className="cp-tile__head">
        <div className="cp-tile__label"><h2>{title}</h2>{helper ? (
          <Popover side="bottom" trigger={<button type="button" className="cp-info" aria-label={`About ${title}`}><Icon name="info" size={15} /></button>}>
            <div className="cp-help">{helper}</div>
          </Popover>
        ) : null}</div>
        {action}
      </header>
      <div className="cp-tile__surface">
        <div className="cp-tile__content">{children}</div>
      </div>
    </section>
  );
}

/** Long text gets a full-height editor on the right; every change is kept (autosaved with the copilot). */
function TextSheet({ open, title, value, placeholder, onChange, onClose }: {
  open: boolean; title: string; value: string; placeholder: string; onChange: (text: string) => void; onClose: () => void;
}) {
  const words = wordCount(value);
  return (
    <Sheet open={open} onClose={onClose} title={title} meta={`${words} ${words === 1 ? "word" : "words"}`}>
      <TextEditor value={value} placeholder={placeholder} aria-label={title} autoFocus onChange={(event) => onChange(event.target.value)} />
    </Sheet>
  );
}

function ResumeCard({ doc, onPreview, onReplace }: { doc: StoredDocument; onPreview: () => void; onReplace: () => void }) {
  return (
    <div className="file-card">
      <span className="file-card__icon" aria-hidden="true"><Icon name="doc" size={18} /></span>
      <div className="file-card__text">
        <b title={doc.file_name}>{displayFileName(doc.file_name)}</b>
        <small>{[fileKind(doc.file_name, doc.mime_type), formatBytes(doc.byte_size), `Added ${formatDate(doc.created_at_ms)}`].filter(Boolean).join(" · ")}</small>
      </div>
      <div className="file-card__actions">
        <Button size="sm" variant="ghost" icon="eye" aria-label="Preview resume" title="Preview resume" onClick={onPreview}>Preview</Button>
        <Button size="sm" variant="secondary" onClick={onReplace}>Replace</Button>
      </div>
    </div>
  );
}

/* --------------------------------- page ---------------------------------- */

export function CopilotPage({ copilot, onCreated, onDelete, onDuplicate, onStart, startWithLink }: CopilotPageProps) {
  const system = useStore(systemStore, (state) => state);
  const docs = useStore(libraryStore, (state) => state.docs);
  const libraryLoaded = useStore(libraryStore, (state) => state.loaded);
  const isNew = copilot === null;

  const initial = useMemo<CopilotInput>(() => {
    if (copilot) return { ...copilot, company: copilot.company || copilot.name, resume_id: null, document_ids: [] };
    // A new copilot starts with the model chosen in Settings.
    return {
      name: "", company: null, target_role: null, job_description: null, instructions: null, save_transcript: true,
      llm_provider: system.provider ?? "open_ai", llm_model: system.model ?? "gpt-5.6-luna", resume_id: null, document_ids: [],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [copilot?.id]);

  const [input, setInput] = useState<CopilotInput>(initial);
  const [sourcesReady, setSourcesReady] = useState(isNew);
  const [savedId, setSavedId] = useState<string | null>(copilot?.id ?? null);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [importUrl, setImportUrl] = useState("");
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [sheet, setSheet] = useState<"job" | "guidance" | null>(null);
  const [picking, setPicking] = useState(false);
  const [viewing, setViewing] = useState<StoredDocument | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [companyError, setCompanyError] = useState(false);
  const urlField = useRef<HTMLInputElement>(null);
  const set = <K extends keyof CopilotInput>(key: K, value: CopilotInput[K]) => setInput((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    if (!libraryLoaded) void loadLibrary();
  }, [libraryLoaded]);

  useEffect(() => {
    if (startWithLink) urlField.current?.focus();
  }, [startWithLink]);

  useEffect(() => {
    if (!copilot) return;
    let alive = true;
    void api.copilots.sources(copilot.id).then((result) => {
      if (!alive) return;
      if (result.ok) {
        setInput((current) => ({
          ...current,
          resume_id: result.value.find((d) => d.doc_type === "resume")?.id ?? null,
          document_ids: result.value.filter((d) => d.doc_type !== "resume").map((d) => d.id),
        }));
      }
      setSourcesReady(true);
    });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [copilot?.id]);

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

  // Autosave: 600 ms after the last change, once sources have loaded and there is a company name.
  const company = (input.company ?? "").trim();
  const creating = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const { state: saveState, flush } = useAutosave(
    input,
    async (value) => {
      const name = (value.company ?? "").trim();
      if (!name) {
        // A new copilot waits quietly for its name; an existing one says why it can't save.
        if (!savedId) return true;
        setCompanyError(true);
        return false;
      }
      if (!savedId) {
        if (creating.current) return true;
        creating.current = true;
      }
      const result = await saveCopilot(savedId, { ...value, name, company: name });
      creating.current = false;
      if (!result.ok) {
        toast(result.error, { tone: "bad" });
        return false;
      }
      if (!savedId) {
        setSavedId(result.value.id);
        if (mounted.current) onCreated(result.value);
      }
      return true;
    },
    { enabled: sourcesReady },
  );

  const resumes = docs.filter((doc) => doc.doc_type === "resume");
  const extras = docs.filter((doc) => doc.doc_type === "other");
  const resume = resumes.find((doc) => doc.id === input.resume_id) ?? null;
  const chosenExtras = extras.filter((doc) => input.document_ids.includes(doc.id));
  const modelKey = `${input.llm_provider}|${input.llm_model}`;
  const options = models.some((m) => `${m.provider}|${m.model}` === modelKey) || models.length === 0 ? models : [{ provider: input.llm_provider, model: input.llm_model }, ...models];

  async function addFiles(docType: DocType, paths?: string[]) {
    const chosen = paths ?? (await pickDocumentFiles());
    if (chosen.length === 0) return;
    const report = await ingestPaths(chosen, docType);
    for (const failure of report.failed) toast(failure.error, { tone: "bad" });
    if (report.added.length === 0) return;
    if (docType === "resume") set("resume_id", report.added[0].document_id);
    else setInput((current) => ({ ...current, document_ids: [...new Set([...current.document_ids, ...report.added.map((d) => d.document_id)])] }));
  }

  const dropOver = useFileDrop((paths) => void addFiles("other", paths), !isNew || Boolean(company));

  async function importJobPost() {
    if (!isWebUrl(importUrl)) return;
    setImportError(null);
    setImporting(true);
    const result = await api.documents.fetchJobPost(importUrl.trim());
    setImporting(false);
    if (!result.ok) {
      setImportError(result.error);
      return;
    }
    set("job_description", result.value);
    setImportUrl("");
    toast("Description imported", { tone: "ok" });
  }

  async function start() {
    const name = company;
    if (!name) {
      setCompanyError(true);
      return;
    }
    await flush();
    const current = copilot && !same(input, initial) ? { ...copilot, ...input, name, company: name } : copilot;
    if (current) onStart(current as CopilotSummary);
  }

  useEffect(() => onMessage("start-session", () => void start()));


  return (
    <div className="cp cp--tiles">
      <header className="cp-head">
        <div className="cp-head__inner">
          <div className="cp-head__titles">
            <InlineTitle
              className="cp-title"
              value={input.company ?? ""}
              placeholder="Company"
              autoFocus={isNew && !startWithLink}
              invalid={companyError}
              onCommit={(next) => { setCompanyError(false); set("company", next || null); }}
            />
            <InlineTitle className="cp-role" value={input.target_role ?? ""} placeholder="Role" onCommit={(next) => set("target_role", next || null)} />

          </div>
          <div className="cp-head__actions">
            <label className="cp-model-control"><span>AI model</span>
              <Select aria-label="AI model" value={modelKey} onChange={(e) => { const [provider, ...rest] = e.target.value.split("|"); setInput((c) => ({ ...c, llm_provider: provider as LlmProviderKind, llm_model: rest.join("|") })); }}>
              {options.length === 0 ? <option value={modelKey}>{modelLabel(input.llm_provider, input.llm_model)}</option> : null}
              {options.map((option) => <option key={`${option.provider}|${option.model}`} value={`${option.provider}|${option.model}`}>{modelLabel(option.provider, option.model)}</option>)}
            </Select>
            </label>
            <div className="cp-transcript-control" title="Keeps the conversation and summary in History">
              <span>Save transcript</span>
              <Toggle checked={input.save_transcript} label="Save transcript" onChange={(next) => set("save_transcript", next)} />
            </div>
            <SaveIndicator state={isNew && !savedId ? "idle" : saveState} />
            {savedId && copilot ? (
              <Menu
                trigger={<IconButton icon="more" label="More" />}
                items={[
                  { label: "Duplicate", icon: "duplicate", onSelect: () => onDuplicate({ ...input, name: `${company} copy`, company: `${company} copy` }) },
                  "separator",
                  { label: "Delete", icon: "trash", danger: true, onSelect: () => setConfirmDelete(true) },
                ]}
              />
            ) : null}
            <Button variant="primary" iconRight="arrow" disabled={!savedId} onClick={() => void start()}>Start session</Button>
          </div>
        </div>
        {companyError ? <p className="cp-error" role="alert">Add a company name first.</p> : null}
      </header>

      <div className="cp-body">
        <Tile name="url" title="Job URL" helper="Import the role from its job posting.">
          <div className="cp-url">
            <Icon name="link" size={15} />
            <input ref={urlField} value={importUrl} placeholder="Paste job post URL" aria-label="Job post URL" aria-invalid={importError ? true : undefined}
              onChange={(event) => { setImportUrl(event.target.value); setImportError(null); }}
              onKeyDown={(event) => { if (event.key === "Enter") void importJobPost(); }} />
            <Button size="sm" variant="secondary" busy={importing} disabled={!isWebUrl(importUrl)} onClick={() => void importJobPost()}>Import</Button>
          </div>
          {importError ? <p className="cp-error" role="alert">{importError}</p> : null}
        </Tile>

        <Tile name="description" title="Description" helper="Aside reads this before every answer."
          action={<Button size="sm" variant="ghost" icon="edit" onClick={() => setSheet("job")}>{input.job_description ? "Edit" : "Add"}</Button>}>
          <AnimatePresence mode="wait" initial={false}>
            <motion.div key={importing ? "loading" : "text"} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }}>
              {importing ? <Skeleton lines={5} /> : <RichText text={input.job_description} lines={5} expandable={false} empty="Import a job URL, or add the description yourself." />}
            </motion.div>
          </AnimatePresence>
        </Tile>

        <Tile name="resume" title="Resume" helper="Your experience, ready for each answer.">
          {!sourcesReady ? <Skeleton lines={2} /> : resume ? (
            <ResumeCard doc={resume} onPreview={() => setViewing(resume)} onReplace={() => setPicking(true)} />
          ) : <button type="button" className="file-empty" onClick={() => setPicking(true)}><Icon name="plus" size={15} />Choose a resume</button>}
        </Tile>

        <Tile name="supporting" title="Supporting files" helper="Add notes, project details or other useful context."
          action={<Button size="sm" variant="ghost" onClick={() => setPicking(true)}>Manage{chosenExtras.length ? ` · ${chosenExtras.length}` : ""}</Button>}>
          {chosenExtras.length > 0 ? <ul className="file-chips">
            {chosenExtras.slice(0, 2).map((doc) => <li key={doc.id}>
              <Icon name="note" size={13} /><span title={doc.file_name}>{displayFileName(doc.file_name)}</span>
              <button type="button" aria-label={`Remove ${doc.file_name}`} onClick={() => set("document_ids", input.document_ids.filter((id) => id !== doc.id))}><Icon name="x" size={12} /></button>
            </li>)}
            {chosenExtras.length > 2 ? <li><button type="button" className="cp-more-files" onClick={() => setPicking(true)}>+{chosenExtras.length - 2} more</button></li> : null}
          </ul> : null}
          <div className="dropzone" data-over={dropOver || undefined}>
            <Icon name="upload" size={16} /><span>Drop files or <button type="button" onClick={() => void addFiles("other")}>browse</button></span>
          </div>
        </Tile>

        <Tile name="guidance" title="Guidance" helper={<>Adds to your <a href={hrefFor("settings", "rules")}>Answer rules</a>.</>}
          action={<Button size="sm" variant="ghost" icon="edit" onClick={() => setSheet("guidance")}>{input.instructions ? "Edit" : "Add"}</Button>}>
          <RichText text={input.instructions} lines={5} expandable={false} empty="Set the tone and what to focus on or avoid." />
        </Tile>
      </div>



      <TextSheet open={sheet === "job"} title="Job description" placeholder="Paste or write the job description. Lines starting with - become bullets." value={input.job_description ?? ""} onChange={(text) => set("job_description", text || null)} onClose={() => setSheet(null)} />
      <TextSheet open={sheet === "guidance"} title="Guidance" placeholder="Tone, focus, what to lean on or avoid. Only Aside sees this." value={input.instructions ?? ""} onChange={(text) => set("instructions", text || null)} onClose={() => setSheet(null)} />

      {picking ? (
        <FilePicker
          resumes={resumes}
          extras={extras}
          resumeId={input.resume_id}
          extraIds={input.document_ids}
          onResume={(id) => set("resume_id", id)}
          onExtras={(ids) => set("document_ids", ids)}
          onAdd={(kind) => void addFiles(kind)}
          onClose={() => setPicking(false)}
        />
      ) : null}
      {viewing ? <DocumentViewer doc={viewing} onClose={() => setViewing(null)} /> : null}
      <Dialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        size="sm"
        title={`Delete ${company || "this copilot"}?`}
        footer={<><Button variant="ghost" onClick={() => setConfirmDelete(false)}>Cancel</Button><Button variant="danger" onClick={() => { setConfirmDelete(false); onDelete(); }}>Delete</Button></>}
      >
        <p>Its files stay in your Library. You can undo for a few seconds.</p>
      </Dialog>
    </div>
  );
}

/** Choose which Library files this copilot uses. */
function FilePicker({ resumes, extras, resumeId, extraIds, onResume, onExtras, onAdd, onClose }: {
  resumes: StoredDocument[]; extras: StoredDocument[]; resumeId: string | null; extraIds: string[];
  onResume: (id: string | null) => void; onExtras: (ids: string[]) => void; onAdd: (kind: DocType) => void; onClose: () => void;
}) {
  return (
    <Dialog open onClose={onClose} size="md" title="Choose files"
      footer={<><Button variant="ghost" icon="upload" onClick={() => onAdd("other")}>Add file</Button><span className="grow" /><Button variant="primary" onClick={onClose}>Done</Button></>}>
      <div className="picker">
        <div className="picker__head"><b>Resume</b><Button size="sm" variant="quiet" icon="plus" onClick={() => onAdd("resume")}>Add</Button></div>
        {resumes.length === 0 ? <p className="hm-muted">No resumes in your Library yet.</p> : (
          <ul className="picker__list">
            {resumes.map((doc) => (
              <li key={doc.id}>
                <label className="picker__item">
                  <input type="radio" name="resume" checked={resumeId === doc.id} onChange={() => onResume(doc.id)} />
                  <span className="picker__name">{displayFileName(doc.file_name)}</span>
                  <small>{fileKind(doc.file_name, doc.mime_type)} · {formatBytes(doc.byte_size)}</small>
                </label>
              </li>
            ))}
          </ul>
        )}
        <div className="picker__head"><b>Supporting files</b></div>
        {extras.length === 0 ? <p className="hm-muted">No documents in your Library yet.</p> : (
          <ul className="picker__list">
            {extras.map((doc) => (
              <li key={doc.id}>
                <label className="picker__item">
                  <input type="checkbox" checked={extraIds.includes(doc.id)} onChange={() => onExtras(extraIds.includes(doc.id) ? extraIds.filter((id) => id !== doc.id) : [...extraIds, doc.id])} />
                  <span className="picker__name">{displayFileName(doc.file_name)}</span>
                  <small>{fileKind(doc.file_name, doc.mime_type)} · {formatBytes(doc.byte_size)}</small>
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
