import { useDocuments } from "../../state/documents";
import { DocumentSection } from "../documents/DocumentSection";

export interface ResumeLayerProps {
  sessionId: string | null;
}

/**
 * Resume — its own sidebar destination (2026-08-30, owner request: "Resume
 * and Additional Documents are separate sections", after a competitor
 * screenshot showing "CVs & Resumes" and "Documents" as two distinct nav
 * items). Was one card on the combined Documents screen; split out into a
 * real screen of its own rather than just visually separated, since the
 * owner's follow-up made clear a shared screen wasn't the ask. Shares
 * `useDocuments` with `DocumentsLayer`/the Create screen's quick-access
 * modals — a resume added from any of the three shows up in all of them.
 */
export function ResumeLayer({ sessionId }: ResumeLayerProps) {
  const documents = useDocuments(sessionId);

  return (
    <>
      {documents.error ? <p className="sc-error-line">{documents.error}</p> : null}
      <DocumentSection title="Resume library" eyebrow="Your reusable background" description="Keep your resume ready for every Copilot. Upload it once, then choose it in any call setup." docType="resume" documents={documents} />
    </>
  );
}
