import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "../../bridge/api";
import { isTauriRuntime } from "../../bridge/runtime";
import type { DocType, StoredDocument } from "../../bridge/types";
import { formatBytes, formatRelative, plural } from "../../core/format";
import { useStore } from "../../core/store";
import { Callout, EmptyState } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Field, TextArea, TextInput } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { toast } from "../../kit/toast";
import { deleteWithUndo } from "../../kit/undo";
import { ingestPaths, libraryStore, loadLibrary, pasteDocument, pickDocumentFiles, setHidden } from "../data";
import { DocumentViewer } from "./DocumentViewer";

const COPY: Record<"resume" | "other", { title: string; noun: string; empty: string; hint: string; pasteName: string }> = {
  resume: {
    title: "Resumes",
    noun: "resume",
    empty: "No resumes yet",
    hint: "Used to write your answers.",
    pasteName: "Resume",
  },
  other: {
    title: "Documents",
    noun: "document",
    empty: "No documents yet",
    hint: "Notes and extra background.",
    pasteName: "Notes",
  },
};

/** Lets people drop PDF or Word files onto the window. Only exists in the real app. */
function useFileDrop(onFiles: (paths: string[]) => void): boolean {
  const [over, setOver] = useState(false);
  const latest = useRef(onFiles);
  latest.current = onFiles;
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") setOver(true);
        else if (payload.type === "leave") setOver(false);
        else if (payload.type === "drop") {
          setOver(false);
          latest.current(payload.paths);
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else stop = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);
  return over;
}

export function LibraryPage({ filter }: { filter: string | null }) {
  const kind: "resume" | "other" = filter === "documents" ? "other" : "resume";
  const { docs, loaded, error, hiddenIds } = useStore(libraryStore, (state) => state);
  const copy = COPY[kind];
  const docType: DocType = kind;
  const [query, setQuery] = useState("");
  const [viewing, setViewing] = useState<StoredDocument | null>(null);
  const [pasting, setPasting] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void loadLibrary();
  }, []);

  const shown = useMemo(() => {
    const term = query.trim().toLowerCase();
    return docs.filter((doc) => doc.doc_type === kind && !hiddenIds.includes(doc.id) && (!term || doc.file_name.toLowerCase().includes(term)));
  }, [docs, kind, hiddenIds, query]);
  const total = docs.filter((doc) => doc.doc_type === kind && !hiddenIds.includes(doc.id)).length;

  async function addPaths(paths: string[]) {
    if (paths.length === 0) return;
    setBusy(true);
    const report = await ingestPaths(paths, docType);
    setBusy(false);
    if (report.added.length > 0) toast(`Added ${plural(report.added.length, copy.noun)}.`, { tone: "ok" });
    for (const failure of report.failed) toast(failure.error, { tone: "bad" });
  }

  const over = useFileDrop((paths) => void addPaths(paths));

  function remove(doc: StoredDocument) {
    deleteWithUndo({
      label: `“${doc.file_name}”`,
      hide: () => setHidden(libraryStore, doc.id, true),
      restore: () => setHidden(libraryStore, doc.id, false),
      commit: async () => {
        const result = await api.documents.remove(doc.id);
        if (result.ok) {
          await loadLibrary();
          setHidden(libraryStore, doc.id, false);
        }
        return result.ok;
      },
      onFailed: (message) => toast(message, { tone: "bad" }),
    });
  }

  return (
    <div className="hm-page">
      <header className="hm-page__head">
        <div>
          <h1>{copy.title}</h1>
          <p className="hm-muted">{copy.hint}</p>
        </div>
        <div className="hm-page__actions">
          <Button variant="secondary" icon="note" onClick={() => setPasting(true)}>Paste text</Button>
          <Button variant="primary" icon="upload" busy={busy} onClick={() => void pickDocumentFiles().then(addPaths)}>Add file</Button>
        </div>
      </header>

      {error ? <Callout tone="bad" title="Couldn't load your library">{error}</Callout> : null}

      {total > 0 ? (
        <label className="hm-search hm-search--wide">
          <Icon name="search" size={16} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search" aria-label={`Find a ${copy.noun}`} />
          <span className="hm-search__count">{plural(total, copy.noun)}</span>
        </label>
      ) : null}

      {loaded && total === 0 ? (
        <EmptyState icon={kind === "resume" ? "doc" : "note"} title={copy.empty} action={<Button variant="primary" icon="upload" onClick={() => void pickDocumentFiles().then(addPaths)}>Add a file</Button>}>
          PDF or Word. Drop files here or paste text.
        </EmptyState>
      ) : (
        <ul className="hm-files">
          {shown.map((doc) => (
            <li key={doc.id} className="hm-file">
              <button type="button" className="hm-file__main" onClick={() => setViewing(doc)}>
                <span className="hm-file__icon"><Icon name="doc" size={18} /></span>
                <span className="hm-file__text">
                  <b>{doc.file_name}</b>
                  <small>
                    {[doc.mime_type?.includes("pdf") ? "PDF" : doc.mime_type?.includes("word") ? "Word" : "Text", formatBytes(doc.byte_size), `Added ${formatRelative(doc.created_at_ms)}`].filter(Boolean).join(" · ")}
                  </small>
                </span>
              </button>
              <Button size="sm" variant="ghost" icon="eye" onClick={() => setViewing(doc)}>View</Button>
              <button type="button" className="icon-btn icon-btn--sm icon-btn--danger" aria-label={`Delete ${doc.file_name}`} title="Delete" onClick={() => remove(doc)}>
                <Icon name="trash" size={16} />
              </button>
            </li>
          ))}
          {loaded && shown.length === 0 && total > 0 ? <li className="hm-muted hm-pad">Nothing matches “{query}”.</li> : null}
        </ul>
      )}

      {over ? <div className="hm-drop" aria-hidden="true"><Icon name="upload" size={26} />Drop PDF or Word files to add them</div> : null}
      {viewing ? <DocumentViewer doc={viewing} onClose={() => setViewing(null)} /> : null}
      {pasting ? (
        <PasteDialog
          docType={docType}
          defaultName={copy.pasteName}
          onClose={() => setPasting(false)}
          onSaved={() => {
            setPasting(false);
            toast("Saved.", { tone: "ok" });
          }}
        />
      ) : null}
    </div>
  );
}

function PasteDialog({ docType, defaultName, onClose, onSaved }: { docType: DocType; defaultName: string; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(defaultName);
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setSaving(true);
    setError(null);
    const result = await pasteDocument(docType, name.trim() || defaultName, text);
    setSaving(false);
    if (result.ok) onSaved();
    else setError(result.error);
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title="Paste text"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" busy={saving} disabled={!text.trim()} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="hm-form">
        <Field label="Name">{(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}</Field>
        <Field label="Text">{(id) => <TextArea id={id} rows={10} value={text} onChange={(event) => setText(event.target.value)} placeholder="Paste here" data-autofocus />}</Field>
        {error ? <Callout tone="bad">{error}</Callout> : null}
      </div>
    </Dialog>
  );
}
