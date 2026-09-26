import { useEffect, useRef, useState } from "react";
import * as api from "../../bridge/api";
import type { DocumentOriginal, StoredDocument } from "../../bridge/types";
import { Callout } from "../../kit/Bits";
import { Dialog } from "../../kit/Dialog";
import { Segmented } from "../../kit/Form";
import { Spinner } from "../../kit/Bits";
import { displayFileName } from "../../core/text";

function fromBase64(value: string): Uint8Array<ArrayBuffer> {
  const raw = atob(value);
  const bytes = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

function PdfPreview({ original }: { original: DocumentOriginal }) {
  const [url, setUrl] = useState("");
  useEffect(() => {
    const next = URL.createObjectURL(new Blob([fromBase64(original.bytes_base64)], { type: original.mime_type }));
    setUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [original]);
  return url ? <object className="hm-pdf" data={url} type="application/pdf">This PDF can't be previewed here. Use “Parsed text”.</object> : null;
}

function DocxPreview({ original }: { original: DocumentOriginal }) {
  const host = useRef<HTMLDivElement>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!host.current) return;
    host.current.replaceChildren();
    setError(false);
    const target = host.current;
    // Loaded only when a Word file is opened; the library is large and most sessions never need it.
    void import("docx-preview")
      .then(({ renderAsync }) => renderAsync(fromBase64(original.bytes_base64), target, undefined, { inWrapper: false, ignoreLastRenderedPageBreak: false }))
      .catch(() => setError(true));
  }, [original]);
  return error ? <Callout tone="warn">Can't show this Word file. See the parsed text.</Callout> : <div ref={host} className="hm-docx" />;
}

/** A read-only look at a saved file: the original as uploaded, or the plain text Aside extracted from it. */
export function DocumentViewer({ doc, onClose }: { doc: StoredDocument; onClose: () => void }) {
  const [view, setView] = useState<"original" | "text">(doc.original_available ? "original" : "text");
  const [original, setOriginal] = useState<DocumentOriginal | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (view !== "original" || !doc.original_available || original) return;
    let alive = true;
    void api.documents.original(doc.id).then((result) => {
      if (!alive) return;
      if (result.ok) setOriginal(result.value);
      else setError(result.error);
    });
    return () => {
      alive = false;
    };
  }, [view, doc, original]);

  return (
    <Dialog open onClose={onClose} size="xl" title={displayFileName(doc.file_name)} subtitle={doc.file_name}>
      {doc.original_available ? (
        <div className="hm-viewer__tabs">
          <Segmented label="What to show" value={view} onChange={setView} options={[{ value: "original", label: "Original file" }, { value: "text", label: "Parsed text" }]} />
        </div>
      ) : (
        <Callout tone="info">Saved as text only.</Callout>
      )}
      <div className="hm-viewer">
        {view === "text" ? (
          <pre className="hm-viewer__text selectable">{doc.raw_text}</pre>
        ) : error ? (
          <Callout tone="bad" title="Couldn't open the original">{error}</Callout>
        ) : !original ? (
          <p className="hm-muted hm-center"><Spinner /> Preparing a private preview…</p>
        ) : original.mime_type === "application/pdf" ? (
          <PdfPreview original={original} />
        ) : (
          <DocxPreview original={original} />
        )}
      </div>
    </Dialog>
  );
}
