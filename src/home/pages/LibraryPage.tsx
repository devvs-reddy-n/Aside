import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import * as api from "../../bridge/api";
import type { DocType, StoredDocument } from "../../bridge/types";
import { formatBytes, formatDate, plural } from "../../core/format";
import { useStore } from "../../core/store";
import { displayFileName, fileKind, guessDocType } from "../../core/text";
import { Callout, EmptyState } from "../../kit/Bits";
import { Button, IconButton } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Field, Segmented, TextInput } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { Menu } from "../../kit/Menu";
import { FAST } from "../../kit/motion";
import { TextEditor } from "../../kit/TextEditor";
import { toast } from "../../kit/toast";
import { deleteWithUndo } from "../../kit/undo";
import { onMessage } from "../bus";
import { copilotsStore, ingestPaths, libraryStore, loadLibrary, pasteDocument, pickDocumentFiles, setHidden } from "../data";
import { useFileDrop } from "../fileDrop";
import { navigate } from "../router";
import { DocumentViewer } from "./DocumentViewer";

type Filter = "all" | "resumes" | "documents";
type View = "list" | "grid";
const VIEW_KEY = "aside-library-view";

function readView(): View {
  try {
    return window.localStorage.getItem(VIEW_KEY) === "grid" ? "grid" : "list";
  } catch {
    return "list";
  }
}

/** How many copilots use each file (worked out from every copilot's attached files). */
function useUsage(): Map<string, number> {
  const copilots = useStore(copilotsStore, (state) => state.items);
  const [usage, setUsage] = useState<Map<string, number>>(new Map());
  const stamp = copilots.map((c) => `${c.id}:${c.updated_at_ms}`).join(",");
  useEffect(() => {
    let alive = true;
    void Promise.all(copilots.map((copilot) => api.copilots.sources(copilot.id))).then((results) => {
      if (!alive) return;
      const counts = new Map<string, number>();
      for (const result of results) {
        if (!result.ok) continue;
        for (const doc of result.value) counts.set(doc.id, (counts.get(doc.id) ?? 0) + 1);
      }
      setUsage(counts);
    });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stamp]);
  return usage;
}

function meta(doc: StoredDocument): string {
  return [doc.doc_type === "resume" ? "Resume" : "Document", fileKind(doc.file_name, doc.mime_type), formatBytes(doc.byte_size), formatDate(doc.created_at_ms)].filter(Boolean).join(" · ");
}

function FileItem({ doc, uses, view, onOpen, onDelete }: { doc: StoredDocument; uses: number; view: View; onOpen: () => void; onDelete: () => void }) {
  return (
    <motion.li
      layout="position"
      className="lib-item"
      data-view={view}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, height: 0, marginTop: 0 }}
      transition={{ duration: FAST }}
    >
      <button type="button" className="lib-item__main" onClick={onOpen}>
        <span className="lib-item__icon" data-kind={doc.doc_type} aria-hidden="true"><Icon name={doc.doc_type === "resume" ? "doc" : "note"} size={18} /></span>
        <span className="lib-item__text">
          <b title={doc.file_name}>{displayFileName(doc.file_name)}</b>
          <small>{meta(doc)}</small>
          <small className="lib-item__uses">{uses > 0 ? `Used by ${plural(uses, "copilot")}` : "Not used yet"}</small>
        </span>
      </button>
      <div className="lib-item__menu">
        <Menu
          trigger={<IconButton icon="more" label={`More for ${displayFileName(doc.file_name)}`} size="sm" />}
          items={[
            { label: "Open", icon: "eye", onSelect: onOpen },
            "separator",
            { label: "Delete", icon: "trash", danger: true, onSelect: onDelete },
          ]}
        />
      </div>
    </motion.li>
  );
}

export function LibraryPage({ filter: param }: { filter: string | null }) {
  const filter: Filter = param === "resumes" || param === "documents" ? param : "all";
  const { docs, loaded, error, hiddenIds } = useStore(libraryStore, (state) => state);
  const usage = useUsage();
  const [view, setView] = useState<View>(readView);
  const [query, setQuery] = useState("");
  const [viewing, setViewing] = useState<StoredDocument | null>(null);
  const [pasting, setPasting] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void loadLibrary();
  }, []);

  const visible = useMemo(() => docs.filter((doc) => !hiddenIds.includes(doc.id)), [docs, hiddenIds]);
  const counts = { all: visible.length, resumes: visible.filter((d) => d.doc_type === "resume").length, documents: visible.filter((d) => d.doc_type !== "resume").length };
  const shown = useMemo(() => {
    const term = query.trim().toLowerCase();
    return visible
      .filter((doc) => filter === "all" || (filter === "resumes") === (doc.doc_type === "resume"))
      .filter((doc) => !term || doc.file_name.toLowerCase().includes(term))
      .sort((a, b) => b.created_at_ms - a.created_at_ms);
  }, [visible, filter, query]);

  /** The chosen filter says what new files are; under All, each file's name decides. */
  async function addPaths(paths: string[]) {
    if (paths.length === 0) return;
    setBusy(true);
    const groups = new Map<DocType, string[]>();
    for (const path of paths) {
      const type: DocType = filter === "resumes" ? "resume" : filter === "documents" ? "other" : guessDocType(path);
      groups.set(type, [...(groups.get(type) ?? []), path]);
    }
    let added = 0;
    for (const [type, group] of groups) {
      const report = await ingestPaths(group, type);
      added += report.added.length;
      for (const failure of report.failed) toast(failure.error, { tone: "bad" });
    }
    setBusy(false);
    if (added > 0) toast(`Added ${plural(added, "file")}`, { tone: "ok" });
  }

  const pick = () => void pickDocumentFiles().then(addPaths);
  const over = useFileDrop((paths) => void addPaths(paths));
  useEffect(() => onMessage("add-files", pick));

  function remove(doc: StoredDocument) {
    deleteWithUndo({
      label: `“${displayFileName(doc.file_name)}”`,
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

  function setLayout(next: View) {
    setView(next);
    try {
      window.localStorage.setItem(VIEW_KEY, next);
    } catch {
      // Not remembered; still applied for now.
    }
  }

  return (
    <div className="hm-page lib">
      <header className="pg-head">
        <h1>Library <span className="pg-count tabular">{counts.all}</span></h1>
        <div className="pg-head__actions">
          <Button variant="secondary" icon="note" onClick={() => setPasting(true)}>Paste text</Button>
          <Button variant="primary" icon="upload" busy={busy} onClick={pick}>Add files</Button>
        </div>
      </header>

      {error ? <Callout tone="bad" title="Couldn't load your library">{error}</Callout> : null}

      <div className="lib-tools">
        <Segmented<Filter>
          label="Show"
          size="sm"
          value={filter}
          onChange={(next) => navigate("library", next === "all" ? undefined : next)}
          options={[
            { value: "all", label: <>All <span className="seg__count">{counts.all}</span></> },
            { value: "resumes", label: <>Resumes <span className="seg__count">{counts.resumes}</span></> },
            { value: "documents", label: <>Documents <span className="seg__count">{counts.documents}</span></> },
          ]}
        />
        {counts.all > 6 ? (
          <label className="hm-search lib-search">
            <Icon name="search" size={15} />
            <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search files" aria-label="Search files" />
          </label>
        ) : <span className="grow" />}
        <Segmented<View>
          label="Layout"
          size="sm"
          value={view}
          onChange={setLayout}
          options={[
            { value: "list", label: <Icon name="list" size={15} />, title: "List" },
            { value: "grid", label: <Icon name="grid" size={15} />, title: "Grid" },
          ]}
        />
      </div>

      {loaded && counts.all === 0 ? (
        <EmptyState
          icon="library"
          title="Your files live here"
          action={<div className="empty__actions"><Button variant="primary" icon="upload" onClick={pick}>Add files</Button><Button variant="secondary" icon="note" onClick={() => setPasting(true)}>Paste text</Button></div>}
        >
          Resumes and notes your copilots can draw on. PDF or Word, or drop them anywhere on this page.
        </EmptyState>
      ) : (
        <ul className="lib-list" data-view={view}>
          <AnimatePresence initial={false}>
            {shown.map((doc) => (
              <FileItem key={doc.id} doc={doc} uses={usage.get(doc.id) ?? 0} view={view} onOpen={() => setViewing(doc)} onDelete={() => remove(doc)} />
            ))}
          </AnimatePresence>
          {loaded && shown.length === 0 ? (
            <li className="lib-none">{query ? <>Nothing matches “{query}”.</> : filter === "resumes" ? "No resumes yet." : "No documents yet."}</li>
          ) : null}
        </ul>
      )}

      <AnimatePresence>
        {over ? (
          <motion.div className="hm-drop" aria-hidden="true" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }}>
            <Icon name="upload" size={24} />
            Drop to add {filter === "resumes" ? "resumes" : filter === "documents" ? "documents" : "files"}
          </motion.div>
        ) : null}
      </AnimatePresence>
      {viewing ? <DocumentViewer doc={viewing} onClose={() => setViewing(null)} /> : null}
      {pasting ? (
        <PasteDialog
          initialType={filter === "resumes" ? "resume" : "other"}
          onClose={() => setPasting(false)}
          onSaved={() => {
            setPasting(false);
            toast("Saved to Library", { tone: "ok" });
          }}
        />
      ) : null}
    </div>
  );
}

function PasteDialog({ initialType, onClose, onSaved }: { initialType: DocType; onClose: () => void; onSaved: () => void }) {
  const [docType, setDocType] = useState<DocType>(initialType);
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fallback = docType === "resume" ? "Resume" : "Notes";

  async function save() {
    setSaving(true);
    setError(null);
    const result = await pasteDocument(docType, name.trim() || fallback, text);
    setSaving(false);
    if (result.ok) onSaved();
    else setError(result.error);
  }

  return (
    <Dialog
      open
      onClose={onClose}
      size="lg"
      title="Paste text"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" busy={saving} disabled={!text.trim()} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="paste">
        <div className="paste__row">
          <Field label="Name">{(id) => <TextInput id={id} value={name} placeholder={fallback} onChange={(event) => setName(event.target.value)} />}</Field>
          <Segmented<DocType> label="Type" value={docType} onChange={setDocType} options={[{ value: "resume", label: "Resume" }, { value: "other", label: "Document" }]} />
        </div>
        <TextEditor value={text} placeholder="Paste here" aria-label="Text" autoFocus onChange={(event) => setText(event.target.value)} />
        {error ? <Callout tone="bad">{error}</Callout> : null}
      </div>
    </Dialog>
  );
}
