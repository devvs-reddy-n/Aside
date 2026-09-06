import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type { ContextPreview, DocType, IngestedDocument, StoredDocument } from "../platform/types";

/** Every doc type this build knows about, used as a fallback before
 *  `list_doc_types` has answered once — mirrors `DocType::ALL`
 *  (`ingest/mod.rs`) so the very first render already lists every kind. */
const FALLBACK_DOC_TYPES: DocType[] = ["resume", "job_description", "other"];

/** How many of each doc type to ask for. A resume/job/notes collection
 *  for one person is never anywhere near this many rows. */
const RECENT_LIMIT = 100;

export interface DocumentsState {
  tauri: boolean;
  /** Every uploaded or pasted document, newest first, across every type. */
  documents: StoredDocument[];
  loading: boolean;
  error: string | null;
  /** A truthful readout of what's actually loaded right now, from
   *  `get_context_preview` — the Documents layer's readiness card is
   *  built from this, never a re-derived guess. */
  preview: ContextPreview | null;
  refresh: () => Promise<void>;
  /** Opens the native file picker, extracts text from the chosen file
   *  (PDF/DOCX), and stores it. `null` means the owner cancelled the
   *  picker or the upload failed (`error` is set for the latter). */
  uploadFile: (docType: DocType) => Promise<IngestedDocument | null>;
  /** Stores pasted plain text as a document — the first-class path for a
   *  job posting copied from a web page, no file needed. */
  pasteText: (docType: DocType, name: string, text: string) => Promise<IngestedDocument | null>;
  remove: (documentId: string) => Promise<boolean>;
}

/**
 * Everything loaded into Aside's memory of the owner: uploaded files,
 * pasted text, and the plain readiness readout for whether enough is
 * loaded to unlock the cheaper repeat-question rate.
 *
 * `sessionId` is the session to attach new uploads to, or `null` to store
 * them with no session yet — verified against `ingest_document`'s own doc
 * comment: uploading before a session exists is a real, supported path,
 * not a misuse. Whatever is loaded here is what `get_context_preview` and
 * a real answer both read from, via `session_id` falling back to the
 * active session on the Rust side.
 */
export function useDocuments(sessionId: string | null): DocumentsState {
  const tauri = isTauriRuntime();
  const [documents, setDocuments] = useState<StoredDocument[]>([]);
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setLoading(true);
    setError(null);

    const typesResult = await invoke<DocType[]>("list_doc_types");
    const types = typesResult.ok && typesResult.value.length > 0 ? typesResult.value : FALLBACK_DOC_TYPES;

    const lists = await Promise.all(
      types.map((docType) => invoke<StoredDocument[]>("list_recent_documents", { docType, limit: RECENT_LIMIT })),
    );
    const merged: StoredDocument[] = [];
    let firstError: string | null = null;
    for (const result of lists) {
      if (result.ok) merged.push(...result.value);
      else firstError = firstError ?? result.error;
    }
    merged.sort((a, b) => b.created_at_ms - a.created_at_ms);
    setDocuments(merged);

    const previewResult = await invoke<ContextPreview>("get_context_preview", { sessionId });
    if (previewResult.ok) setPreview(previewResult.value);
    else firstError = firstError ?? previewResult.error;

    setError(firstError);
    setLoading(false);
  }, [sessionId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const uploadFile = useCallback(
    async (docType: DocType): Promise<IngestedDocument | null> => {
      if (!isTauriRuntime()) return null;
      setError(null);
      const path = await open({
        title: "Choose a file",
        multiple: false,
        filters: [{ name: "Resume or document", extensions: ["pdf", "docx"] }],
      });
      if (!path) return null;
      const result = await invoke<IngestedDocument>("ingest_document", {
        sessionId,
        path,
        docType,
      });
      if (!result.ok) {
        setError(result.error);
        return null;
      }
      await refresh();
      return result.value;
    },
    [sessionId, refresh],
  );

  const pasteText = useCallback(
    async (docType: DocType, name: string, text: string): Promise<IngestedDocument | null> => {
      setError(null);
      const result = await invoke<IngestedDocument>("ingest_text_document", {
        sessionId,
        docType,
        name,
        text,
      });
      if (!result.ok) {
        setError(result.error);
        return null;
      }
      await refresh();
      return result.value;
    },
    [sessionId, refresh],
  );

  const remove = useCallback(
    async (documentId: string): Promise<boolean> => {
      setError(null);
      const result = await invoke<boolean>("delete_document", { documentId });
      if (!result.ok) {
        setError(result.error);
        return false;
      }
      await refresh();
      return true;
    },
    [refresh],
  );

  return { tauri, documents, loading, error, preview, refresh, uploadFile, pasteText, remove };
}
