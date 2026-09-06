import type { StoredDocument } from "../../platform/types";

/** Which step of the standalone window is showing — the Create form
 *  itself, or Screen B (Connect) once a session has been created. The
 *  window has no other tabs: the old "Call Sessions" dashboard tab and
 *  the Interview/Regular Call segmented control are both gone per the
 *  owner's explicit "remove regular call session and past history of
 *  sessions" — see DESIGN.md §1's replacement table. */
export type CreateStep = "form" | "connect";

/** A resume/document as picked in the Resume/Documents modals — just the
 *  fields the Create form's chips need, kept separate from the full
 *  `StoredDocument` shape so callers don't reach into raw_text by
 *  accident (large blobs of extracted text have no business rendering in
 *  a chip). */
export function docLabel(doc: StoredDocument): string {
  return doc.file_name;
}
