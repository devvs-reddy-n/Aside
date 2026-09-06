import { useEffect, useRef, useState } from "react";
import { renderAsync } from "docx-preview";
import { invoke } from "../../platform/invoke";
import { Icon } from "../../ui";
import type { DocumentOriginal, StoredDocument } from "../../platform/types";

export function DocumentInspector({ document, onClose }: { document: StoredDocument; onClose: () => void }) {
  const [view, setView] = useState<"original" | "text">(document.original_available ? "original" : "text");
  const [original, setOriginal] = useState<DocumentOriginal | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (view !== "original" || !document.original_available || original) return;
    void (async () => {
      const result = await invoke<DocumentOriginal>("read_document_original", { documentId: document.id });
      if (result.ok) setOriginal(result.value); else setError(result.error);
    })();
  }, [view, document, original]);
  return <div className="di-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="di-panel" aria-label={`View ${document.file_name}`} onMouseDown={(event) => event.stopPropagation()}>
      <header><div><p>Library file</p><h2>{document.file_name}</h2></div><button type="button" onClick={onClose} aria-label="Close"><Icon name="close" /></button></header>
      <div className="di-tabs" role="tablist">
        {document.original_available ? <button type="button" role="tab" aria-selected={view === "original"} onClick={() => setView("original")}>Original file</button> : null}
        <button type="button" role="tab" aria-selected={view === "text"} onClick={() => setView("text")}>Parsed text</button>
      </div>
      <div className="di-content">
        {view === "text" ? <pre>{document.raw_text}</pre> : error ? <p className="sc-error-line">{error}</p> : !original ? <p className="di-loading">Preparing a private preview…</p> : original.mime_type === "application/pdf" ? <PdfPreview original={original} /> : <DocxPreview original={original} />}
      </div>
      {!document.original_available ? <footer>This older library item has parsed text only. Re-upload it to retain and view its original file.</footer> : null}
    </section>
  </div>;
}

function fromBase64(value: string): Uint8Array { const raw = atob(value); return Uint8Array.from(raw, (char) => char.charCodeAt(0)); }
function PdfPreview({ original }: { original: DocumentOriginal }) {
  const [url, setUrl] = useState("");
  useEffect(() => { const next = URL.createObjectURL(new Blob([fromBase64(original.bytes_base64)], { type: original.mime_type })); setUrl(next); return () => URL.revokeObjectURL(next); }, [original]);
  return url ? <object className="di-pdf" data={url} type="application/pdf">This PDF cannot be previewed in this window.</object> : null;
}
function DocxPreview({ original }: { original: DocumentOriginal }) {
  const ref = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (!ref.current) return; ref.current.replaceChildren(); void renderAsync(fromBase64(original.bytes_base64), ref.current, undefined, { inWrapper: false, ignoreLastRenderedPageBreak: false }).catch(() => setError("Aside could not render this DOCX. Its parsed text is still available.")); }, [original]);
  return error ? <p className="sc-error-line">{error}</p> : <div ref={ref} className="di-docx" />;
}
