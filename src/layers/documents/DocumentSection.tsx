import { useState } from "react";
import { Badge, Icon, Table, TableRow } from "../../ui";
import type { useDocuments } from "../../state/documents";
import type { DocType, StoredDocument } from "../../platform/types";
import { DocumentInspector } from "./DocumentInspector";

export interface DocumentSectionProps {
  title: string;
  eyebrow: string;
  description: string;
  docType: DocType;
  documents: ReturnType<typeof useDocuments>;
}

function fileTypeLabel(mimeType: string | null): string {
  if (!mimeType) return "TEXT";
  if (mimeType.includes("pdf")) return "PDF";
  if (mimeType.includes("word") || mimeType.includes("officedocument")) return "DOCX";
  return mimeType.split("/").pop()?.toUpperCase() ?? "FILE";
}

function fileSizeLabel(bytes: number | null): string {
  if (bytes == null) return "Not available";
  if (bytes < 1024) return `${bytes} B`;
  return `${Math.round(bytes / 1024)} KB`;
}

function dateLabel(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

/** One full-width library page for a document type. It shares the same
 * list-first structure as Copilots so uploading and finding saved files are
 * equally clear in both the Resume and Documents destinations. */
export function DocumentSection({ title, eyebrow, description, docType, documents }: DocumentSectionProps) {
  const [uploading, setUploading] = useState(false);
  const [viewing, setViewing] = useState<StoredDocument | null>(null);
  const rows = documents.documents.filter((doc) => doc.doc_type === docType);

  async function upload() {
    setUploading(true);
    await documents.uploadFile(docType);
    setUploading(false);
  }

  const itemName = docType === "resume" ? "resume" : "document";

  return (
    <section className="dc-page">
      <header className="dc-hero">
        <div>
          <p className="dc-kicker">{eyebrow}</p>
          <h1>{title}</h1>
          <p>{description}</p>
        </div>
        <button type="button" className="cp-primary" disabled={uploading} onClick={() => void upload()}>
          <Icon name="plus" /> {uploading ? "Uploading…" : `Upload ${itemName}`}
        </button>
      </header>
      <div className="dc-toolbar">
        <span className="dc-count">
          {rows.length} saved {rows.length === 1 ? "file" : "files"}
        </span>
      </div>

      {rows.length === 0 ? (
        <div className="dc-empty">
          <span className="dc-empty__mark"><Icon name="document" size={22} /></span>
          <h2>Upload your first {itemName}</h2>
          <p>{docType === "resume" ? "Add a resume to use it in any Copilot." : "Add notes, briefs, or reference files to use in your Copilots."}</p>
          <button type="button" className="cp-primary" disabled={uploading} onClick={() => void upload()}><Icon name="plus" /> Upload {itemName}</button>
        </div>
      ) : (
        <Table className="dc-table" ariaLabel={title}>
          <TableRow head className="dc-table__row dc-table__row--head">
            <span role="columnheader" className="dc-table__name-head">
              <Icon name="document" size={15} className="u-invisible-spacer" />
              Name
            </span>
            <span role="columnheader">Size</span>
            <span role="columnheader">Source</span>
            <span role="columnheader">File Type</span>
            <span role="columnheader">Created</span>
            <span role="columnheader" aria-hidden="true" />
          </TableRow>
          {rows.map((doc) => (
            <DocumentTableRow key={doc.id} doc={doc} onOpen={() => setViewing(doc)} onRemove={() => documents.remove(doc.id)} />
          ))}
        </Table>
      )}
      {viewing ? <DocumentInspector document={viewing} onClose={() => setViewing(null)} /> : null}
    </section>
  );
}

function DocumentTableRow({ doc, onOpen, onRemove }: { doc: StoredDocument; onOpen: () => void; onRemove: () => void }) {
  return (
    <TableRow className="dc-table__row">
      <button type="button" className="dc-table__name" onClick={onOpen}>
        <Icon name="document" size={15} />
        {doc.file_name}
      </button>
      <span>{fileSizeLabel(doc.byte_size)}</span>
      <span>
        <Badge tone={doc.original_available ? "accent" : "neutral"}>
          {doc.original_available ? "Uploaded" : "Created Manually"}
        </Badge>
      </span>
      <span>{fileTypeLabel(doc.mime_type)}</span>
      <span>{dateLabel(doc.created_at_ms)}</span>
      <span className="dc-table__actions">
        <button type="button" className="dc-row__remove" aria-label={`View ${doc.file_name}`} onClick={onOpen}>
          <Icon name="download" size={14} />
        </button>
        <button type="button" className="dc-row__remove" aria-label={`Remove ${doc.file_name}`} onClick={onRemove}>
          <Icon name="trash" size={14} />
        </button>
      </span>
    </TableRow>
  );
}
