import { useState } from "react";
import type { DocType, StoredDocument } from "../../bridge/types";
import { formatBytes } from "../../core/format";
import { displayFileName, fileKind } from "../../core/text";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Icon } from "../../kit/Icon";
import { Popover } from "../../kit/Popover";
import { Skeleton } from "../../kit/Skeleton";
import { SetupSection } from "../SetupSection";

interface Props {
  resumes: StoredDocument[];
  documents: StoredDocument[];
  resumeId: string | null;
  documentIds: string[];
  loaded: boolean;
  busy: boolean;
  dropOver: boolean;
  onResume: (id: string | null) => void;
  onDocuments: (ids: string[]) => void;
  onAdd: (kind: DocType) => Promise<void>;
  onPreview: (doc: StoredDocument) => void;
}

/** One attachment group, with a single resume and an independent multi-select. */
export function CopilotDocuments({ resumes, documents, resumeId, documentIds, loaded, busy, dropOver, onResume, onDocuments, onAdd, onPreview }: Props) {
  const [replacing, setReplacing] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [query, setQuery] = useState("");
  const resume = resumes.find((doc) => doc.id === resumeId);
  const selected = documents.filter((doc) => documentIds.includes(doc.id));
  const names = selected.map((doc) => displayFileName(doc.file_name)).join(", ");
  const matches = documents.filter((doc) => displayFileName(doc.file_name).toLowerCase().includes(query.trim().toLowerCase()));
  const toggle = (id: string) => onDocuments(documentIds.includes(id) ? documentIds.filter((entry) => entry !== id) : [...documentIds, id]);

  return <SetupSection name="documents" title="Documents" icon="folder">
    {!loaded ? <div className="cp-documents cp-documents--loading"><Skeleton lines={3} /></div> : <div className="cp-documents" data-over={dropOver || undefined} aria-busy={busy}>
      <div className="cp-attachment">
        <span className="cp-attachment__icon"><Icon name="file" size={18} /></span>
        <div className="cp-attachment__text">
          <span className="cp-attachment__label">Resume</span>
          {resume ? <button type="button" className="cp-attachment__preview" aria-label="Preview resume" title={`Preview ${resume.file_name}`} disabled={busy} onClick={() => onPreview(resume)}>
            <span>{displayFileName(resume.file_name)}</span>
          </button> : <span className="cp-attachment__value" data-empty>Add your resume</span>}
        </div>
        <div className="cp-attachment__actions">
          <Button size="sm" variant="secondary" icon={resume ? "refresh" : "plus"} disabled={busy} onClick={() => setReplacing(true)}>{resume ? "Replace" : "Choose"}</Button>
        </div>
      </div>
      <div className="cp-attachment">
        <span className="cp-attachment__icon"><Icon name="note" size={18} /></span>
        <div className="cp-attachment__text">
          <span className="cp-attachment__label">Supporting documents</span>
          <span className="cp-attachment__value cp-attachment__files" data-empty={!selected.length || undefined} title={names || undefined}>
            {selected.length ? <><span className="cp-attachment__filename">{displayFileName(selected[0].file_name)}</span>{selected.length > 1 ? <span className="cp-attachment__remaining">, {selected.length - 1} other{selected.length === 2 ? "" : "s"}</span> : null}</> : "No files selected"}
          </span>
        </div>
        <div className="cp-attachment__actions">
          <Popover side="bottom" align="end" open={selecting} onOpenChange={(open) => { setSelecting(open); if (!open) setQuery(""); }}
            trigger={<Button size="sm" variant="secondary" icon="plus" disabled={busy} aria-label="Select supporting documents">Select</Button>}>
            <div className="document-select" aria-label="Supporting documents">
              <header className="document-select__head"><b>Supporting documents</b><span role="status">{selected.length} selected</span></header>
              <input className="input" aria-label="Search supporting documents" placeholder="Search your Library" value={query} onChange={(event) => setQuery(event.target.value)} />
              <div className="document-select__tools">
                <Button size="sm" variant="ghost" disabled={busy || !matches.length} onClick={() => onDocuments([...new Set([...documentIds, ...matches.map((doc) => doc.id)])])}>{query.trim() ? "Select shown" : "Select all"}</Button>
                <Button size="sm" variant="ghost" disabled={busy || !documentIds.length} onClick={() => onDocuments([])}>Clear</Button>
              </div>
              <div className="document-select__list" role="group" aria-label="Choose supporting documents">
                {matches.length ? matches.map((doc) => <label key={doc.id} className="document-choice" data-selected={documentIds.includes(doc.id) || undefined}>
                  <input type="checkbox" checked={documentIds.includes(doc.id)} disabled={busy} onChange={() => toggle(doc.id)} />
                  <Icon name="doc" size={17} />
                  <span><b title={doc.file_name}>{displayFileName(doc.file_name)}</b><small>{fileKind(doc.file_name, doc.mime_type)} / {formatBytes(doc.byte_size)}</small></span>
                </label>) : <p className="document-select__empty">{query.trim() ? "No matching documents." : "Add PDF or Word files to select them here."}</p>}
              </div>
              <footer className="document-select__foot">
                <Button size="sm" variant="ghost" icon="upload" busy={busy} onClick={() => void onAdd("other")}>Add files</Button>
                <Button size="sm" variant="primary" onClick={() => { setSelecting(false); setQuery(""); }}>Done</Button>
              </footer>
            </div>
          </Popover>
        </div>
      </div>
    </div>}
    {replacing ? <Dialog open title={resume ? "Replace resume" : "Choose resume"} size="md" onClose={() => { if (!busy) setReplacing(false); }}
      footer={<><Button variant="ghost" icon="upload" busy={busy} onClick={() => void onAdd("resume")}>Add resume</Button><span className="grow" /><Button variant="primary" disabled={busy} onClick={() => setReplacing(false)}>Done</Button></>}>
      <div className="resume-choices" role="radiogroup" aria-label="Resume">
        <label className="document-choice" data-selected={!resumeId || undefined}>
          <input type="radio" name="resume" checked={!resumeId} disabled={busy} onChange={() => onResume(null)} />
          <Icon name="minus" size={18} />
          <span><b>No resume</b><small>You can add one later.</small></span>
        </label>
        {resumes.map((doc) => <label key={doc.id} className="document-choice" data-selected={resumeId === doc.id || undefined}>
          <input type="radio" name="resume" checked={resumeId === doc.id} disabled={busy} onChange={() => onResume(doc.id)} />
          <Icon name="file" size={18} />
          <span><b title={doc.file_name}>{displayFileName(doc.file_name)}</b><small>{fileKind(doc.file_name, doc.mime_type)} / {formatBytes(doc.byte_size)}</small></span>
        </label>)}
      </div>
    </Dialog> : null}
  </SetupSection>;
}
