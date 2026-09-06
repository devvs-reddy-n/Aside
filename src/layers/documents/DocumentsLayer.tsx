import { useDocuments } from "../../state/documents";
import { DocumentSection } from "./DocumentSection";

export interface DocumentsLayerProps {
  sessionId: string | null;
}

/**
 * Documents — a real, dedicated screen for Additional Documents (notes,
 * briefs, anything else), promoted from the Create screen's quick-access
 * modal (`DocumentsModal`, which stays as-is for in-flow use). Shares
 * `useDocuments` with that modal and with `ResumeLayer`, so a file added
 * from any of them shows up in all of them. Two things this screen used to
 * hold were deliberately moved out (2026-08-30, owner requests, not
 * regressions): Job Description, since it's typed directly on the Create
 * Session screen and was never an upload, so a dropzone for it here just
 * duplicated that with no real use; and Resume, split into its own sidebar
 * destination (`ResumeLayer`) after the owner confirmed Resume and
 * Documents are meant to be separate sections, not two cards sharing one
 * screen — matching a competitor app's own "CVs & Resumes" / "Documents"
 * split the owner pointed to as reference.
 */
export function DocumentsLayer({ sessionId }: DocumentsLayerProps) {
  const documents = useDocuments(sessionId);

  return (
    <>
      {documents.error ? <p className="sc-error-line">{documents.error}</p> : null}
      <DocumentSection title="Documents" eyebrow="Your reusable material" description="Notes, briefs, and reference files you can attach to one or many Copilots." docType="other" documents={documents} />
    </>
  );
}
