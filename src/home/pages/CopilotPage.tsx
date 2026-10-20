import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotInput, CopilotSummary, DocType, LlmProviderKind, StoredDocument } from "../../bridge/types";
import { useStore } from "../../core/store";
import { isWebUrl } from "../../core/text";
import { Button, IconButton } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Select, Toggle } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { Menu } from "../../kit/Menu";
import { FAST } from "../../kit/motion";
import { toast } from "../../kit/toast";
import { useAutosave } from "../../kit/useAutosave";
import type { SaveState } from "../../core/autosave";
import { ingestPaths, libraryStore, loadLibrary, pickDocumentFiles, saveCopilot, systemStore } from "../data";
import { useFileDrop } from "../fileDrop";
import { DEFAULT_MODEL_CHOICE, copilotModelLabel, modelLabel } from "../labels";
import { SetupSummary } from "../SetupSummary";
import { hrefFor } from "../router";
import { CopilotDocuments } from "./CopilotDocuments";
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

function SaveIndicator({ state }: { state: SaveState }) {
  const text = state === "saving" || state === "pending" ? "Saving…" : state === "saved" ? "Saved" : state === "error" ? "Not saved" : "";
  return (
    <span className="save-state" data-state={state} aria-live="polite">
      <AnimatePresence mode="wait" initial={false}>
        {text ? (
          <motion.span key={text} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }}>
            {state === "saved" ? (
              <Icon name="check" size={14} />
            ) : state === "error" ? <Icon name="alertCircle" size={14} /> : null}
            {text}
          </motion.span>
        ) : null}
      </AnimatePresence>
    </span>
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
      llm_provider: system.provider ?? "open_ai", llm_model: DEFAULT_MODEL_CHOICE, kind: "interview", resume_id: null, document_ids: [],
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
  // The job-post link field stays hidden until Add URL is pressed (or the copilot was started from a link).
  const [urlOpen, setUrlOpen] = useState(Boolean(startWithLink));
  const [filesBusy, setFilesBusy] = useState(false);
  const filesLock = useRef(false);
  const [viewing, setViewing] = useState<StoredDocument | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [companyError, setCompanyError] = useState(false);
  const urlField = useRef<HTMLInputElement>(null);
  const set = <K extends keyof CopilotInput>(key: K, value: CopilotInput[K]) => setInput((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    if (!libraryLoaded) void loadLibrary();
  }, [libraryLoaded]);

  useEffect(() => {
    if (startWithLink) setUrlOpen(true);
  }, [startWithLink]);

  function closeUrl() {
    if (importing) return;
    setUrlOpen(false);
    setImportUrl("");
    setImportError(null);
  }

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
  // Only the provider chosen in Settings is offered; a model saved under the other provider follows Default.
  const provider = system.provider ?? "open_ai";
  const options = models.filter((m) => m.provider === provider);
  const usesDefault = input.llm_model === DEFAULT_MODEL_CHOICE || input.llm_provider !== provider || !options.some((m) => m.model === input.llm_model);
  const modelKey = usesDefault ? DEFAULT_MODEL_CHOICE : `${input.llm_provider}|${input.llm_model}`;
  const defaultLabel = copilotModelLabel(provider, DEFAULT_MODEL_CHOICE, system);

  async function addFiles(docType: DocType, paths?: string[]) {
    if (filesLock.current) return;
    filesLock.current = true;
    setFilesBusy(true);
    try {
      const chosen = paths ?? (await pickDocumentFiles());
      if (!chosen.length) return;
      const report = await ingestPaths(docType === "resume" ? chosen.slice(0, 1) : chosen, docType);
      for (const failure of report.failed) toast(failure.error, { tone: "bad" });
      if (!report.added.length) return;
      if (docType === "resume") set("resume_id", report.added[0].document_id);
      else setInput((current) => ({ ...current, document_ids: [...new Set([...current.document_ids, ...report.added.map((d) => d.document_id)])] }));
    } catch {
      toast("Couldn't add those files. Try again.", { tone: "bad" });
    } finally {
      filesLock.current = false;
      setFilesBusy(false);
    }
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
    setUrlOpen(false);
    toast("Description imported", { tone: "ok" });
  }

  async function start() {
    if (importing || filesLock.current || !sourcesReady) return;
    const name = company;
    if (!name) {
      setCompanyError(true);
      return;
    }
    if (!(await flush())) {
      toast("Your latest changes could not be saved. Try again before starting.", { tone: "bad" });
      return;
    }
    const current = copilot && !same(input, initial) ? { ...copilot, ...input, name, company: name } : copilot;
    if (current) onStart(current as CopilotSummary);
  }



  return (
    <div className="cp cp--tiles">
      <div className="cp-scroll">
        <header className="cp-head">
          <div className="cp-head__inner">
            <div className="cp-head__titles"><h1>{isNew ? "New Copilot" : "Copilot setup"}</h1></div>
            <div className="cp-head__actions">
              <label className="cp-model-control" title="AI model">
                <Select aria-label="AI model" value={modelKey} onChange={(e) => {
                  // Default follows the model chosen in Settings, now and after it changes.
                  if (e.target.value === DEFAULT_MODEL_CHOICE) return setInput((c) => ({ ...c, llm_provider: system.provider ?? c.llm_provider, llm_model: DEFAULT_MODEL_CHOICE }));
                  const [provider, ...rest] = e.target.value.split("|");
                  setInput((c) => ({ ...c, llm_provider: provider as LlmProviderKind, llm_model: rest.join("|") }));
                }}>
                <option value={DEFAULT_MODEL_CHOICE}>{defaultLabel}</option>
                {options.map((option) => <option key={`${option.provider}|${option.model}`} value={`${option.provider}|${option.model}`}>{modelLabel(option.provider, option.model)}</option>)}
              </Select>
              </label>
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
            </div>
          </div>
          <div className="setup-identity">
            <label><span>Company</span><input className="input" value={input.company ?? ""} placeholder="Company name" autoFocus={isNew && !startWithLink}
              aria-invalid={companyError || undefined} onChange={(event) => { setCompanyError(false); set("company", event.target.value || null); }} /></label>
            <label><span>Role <small>Optional</small></span><input className="input" value={input.target_role ?? ""} placeholder="Job title" aria-label="Role" onChange={(event) => set("target_role", event.target.value || null)} /></label>
          </div>
          {companyError ? <p className="cp-error" role="alert">Add a company name first.</p> : null}
        </header>

        <div className="cp-body">
          <SetupSummary name="description" title="Job description" icon="doc" copilotId={savedId} kind="job_description" value={input.job_description} hidden={urlOpen}
            placeholder="Add a job description to see five key points." onChange={(value) => set("job_description", value)} helper="Five AI key points. Edit opens the full description, which is used during your session."
            action={<div className="cp-tile__actions">
              <Button size="sm" variant="ghost" icon="link" aria-expanded={urlOpen} onClick={() => (urlOpen ? closeUrl() : setUrlOpen(true))}>Add URL</Button>
            </div>}>
            <AnimatePresence initial={false}>
              {urlOpen ? (
                <motion.div key="url" className="cp-url-wrap" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }}>
                  <div className="cp-url">
                    <Icon name="link" size={15} />
                    <input ref={urlField} autoFocus value={importUrl} placeholder="Paste job post URL to import" aria-label="Job post URL" aria-invalid={importError ? true : undefined}
                      onChange={(event) => { setImportUrl(event.target.value); setImportError(null); }}
                      onKeyDown={(event) => { if (event.key === "Enter") void importJobPost(); else if (event.key === "Escape") { event.stopPropagation(); closeUrl(); } }} />
                    <Button size="sm" variant="secondary" busy={importing} disabled={!isWebUrl(importUrl)} onClick={() => void importJobPost()}>Import</Button>
                  </div>
                  {importError ? <p className="cp-error" role="alert">{importError}</p> : null}
                </motion.div>
              ) : null}
            </AnimatePresence>
          </SetupSummary>

          <CopilotDocuments resumes={resumes} documents={extras} resumeId={input.resume_id} documentIds={input.document_ids}
            loaded={sourcesReady && libraryLoaded} busy={filesBusy} dropOver={dropOver} onResume={(id) => set("resume_id", id)}
            onDocuments={(ids) => set("document_ids", ids)} onAdd={addFiles} onPreview={setViewing} />

          <SetupSummary name="guidance" title="Guidance" icon="bulb" copilotId={savedId} kind="guidance" value={input.instructions}
            helper={<>Adds to your <a href={hrefFor("settings", "rules")}>Answer rules</a>.</>}
            placeholder="Add guidance to see five response preferences." onChange={(value) => set("instructions", value)} />
        </div>
        <footer className="cp-foot">
          <div className="cp-foot__inner">
            <div className="cp-transcript-control" title="Keeps the conversation and summary in History">
              <span>Save transcript</span>
              <Toggle checked={input.save_transcript} label="Save transcript" onChange={(next) => set("save_transcript", next)} />
            </div>
            <SaveIndicator state={isNew && !savedId ? "idle" : saveState} />
            <Button variant="primary" className="cp-start" icon="play" iconRight="arrow" disabled={!savedId || importing || filesBusy || !company || !sourcesReady} onClick={() => void start()}>Start session</Button>

          </div>
        </footer>
      </div>


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
