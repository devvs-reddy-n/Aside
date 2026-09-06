import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type {
  PastSessionSummary,
  TranscriptTurn,
  SessionAnswer,
  SessionSummary,
  TranscriptSearchResult,
} from "../platform/types";

export interface PastSessionsState {
  tauri: boolean;
  sessions: PastSessionSummary[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  getTranscript: (sessionId: string) => Promise<TranscriptTurn[] | null>;
  getAnswers: (sessionId: string) => Promise<SessionAnswer[] | null>;
  getContext: (sessionId: string) => Promise<string[] | null>;
  getSummary: (sessionId: string) => Promise<SessionSummary | null>;
  search: (query: string) => Promise<TranscriptSearchResult[]>;
  remove: (sessionId: string) => Promise<boolean>;
}

/**
 * The Call Sessions screen's whole reason for existing: real, permanent
 * session history. Re-created after the old `past-sessions/` layer was
 * removed per an earlier owner instruction — the backend
 * (`list_past_sessions`/`get_session_transcript`/`search_transcripts`/
 * `delete_session`, `storage/past_sessions.rs`) was kept the whole time
 * and had zero frontend callers until this hook.
 */
export function usePastSessions(): PastSessionsState {
  const tauri = isTauriRuntime();
  const [sessions, setSessions] = useState<PastSessionSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setLoading(true);
    const result = await invoke<PastSessionSummary[]>("list_past_sessions");
    if (result.ok) {
      setSessions(result.value);
      setError(null);
    } else {
      setError(result.error);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const getTranscript = useCallback(async (sessionId: string): Promise<TranscriptTurn[] | null> => {
    const result = await invoke<TranscriptTurn[]>("get_session_transcript", { sessionId });
    if (!result.ok) {
      setError(result.error);
      return null;
    }
    return result.value;
  }, []);

  const getAnswers = useCallback(async (sessionId: string): Promise<SessionAnswer[] | null> => {
    const result = await invoke<SessionAnswer[]>("get_session_answers", { sessionId });
    if (!result.ok) {
      setError(result.error);
      return null;
    }
    return result.value;
  }, []);

  const getContext = useCallback(async (sessionId: string): Promise<string[] | null> => {
    const result = await invoke<string[]>("get_session_context", { sessionId });
    if (!result.ok) {
      setError(result.error);
      return null;
    }
    return result.value;
  }, []);

  const getSummary = useCallback(async (sessionId: string): Promise<SessionSummary | null> => {
    const result = await invoke<SessionSummary>("summarize_past_session", { sessionId });
    if (!result.ok) {
      setError(result.error);
      return null;
    }
    return result.value;
  }, []);

  const search = useCallback(async (query: string): Promise<TranscriptSearchResult[]> => {
    if (query.trim().length === 0) return [];
    const result = await invoke<TranscriptSearchResult[]>("search_transcripts", { query: query.trim() });
    if (!result.ok) {
      setError(result.error);
      return [];
    }
    return result.value;
  }, []);

  const remove = useCallback(
    async (sessionId: string): Promise<boolean> => {
      const result = await invoke<boolean>("delete_session", { sessionId });
      if (!result.ok || !result.value) {
        setError(result.ok ? "That session could not be found." : result.error);
        return false;
      }
      await refresh();
      return true;
    },
    [refresh],
  );

  return { tauri, sessions, loading, error, refresh, getTranscript, getAnswers, getContext, getSummary, search, remove };
}
