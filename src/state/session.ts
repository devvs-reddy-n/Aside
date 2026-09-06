import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import { SESSION_EVENTS } from "../platform/eventNames";
import type { ActiveSessionInfo, Session, SessionType } from "../platform/types";

export interface CreateSessionInput {
  title: string;
  company?: string | null;
  jobDescription?: string | null;
  instructions?: string | null;
  sessionType?: SessionType | null;
  saveTranscript?: boolean | null;
}

export interface SessionState {
  tauri: boolean;
  /** The session currently live for the whole app (both windows read
   *  this), or `null` when nothing is running. Kept in sync with the
   *  backend by `session-started` / `session-ended`, so every component
   *  using this hook sees the same session without polling. */
  active: ActiveSessionInfo | null;
  loading: boolean;
  error: string | null;
  /** Creates a real, saved session row and returns it, or `null` on
   *  failure (`error` is set either way). */
  createSession: (input: CreateSessionInput) => Promise<Session | null>;
  /** Ends a session by id (marks it finished, flushes anything queued for
   *  it). Returns whether it succeeded. */
  endSession: (id: string) => Promise<boolean>;
  refreshActive: () => Promise<void>;
}

/**
 * The one session live across the whole app right now: who it is with,
 * what kind of call it is, and whether Aside should answer automatically.
 * `commands::session::create_session` only creates the row — it does not
 * itself become "the active session"; only
 * `commands::session::start_ephemeral_session` (not wrapped here, this
 * chunk has no caller for it yet) or setting it up elsewhere makes
 * `get_active_session` return something. Later layers extend this rather
 * than replace it.
 */
export function useSession(): SessionState {
  const tauri = isTauriRuntime();
  const [active, setActive] = useState<ActiveSessionInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshActive = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const result = await invoke<ActiveSessionInfo | null>("get_active_session");
    if (result.ok) setActive(result.value);
    else setError(result.error);
  }, []);

  useEffect(() => {
    void refreshActive();
  }, [refreshActive]);

  useTauriEvent<ActiveSessionInfo>(SESSION_EVENTS.started, (payload) => setActive(payload));
  useTauriEvent<null>(SESSION_EVENTS.ended, () => setActive(null));

  const createSession = useCallback(async (input: CreateSessionInput): Promise<Session | null> => {
    setLoading(true);
    setError(null);
    const result = await invoke<Session>("create_session", {
      title: input.title,
      company: input.company ?? null,
      jobDescription: input.jobDescription ?? null,
      instructions: input.instructions ?? null,
      sessionInstructions: null,
      sessionType: input.sessionType ?? null,
      saveTranscript: input.saveTranscript ?? null,
    });
    setLoading(false);
    if (result.ok) return result.value;
    setError(result.error);
    return null;
  }, []);

  const endSession = useCallback(async (id: string): Promise<boolean> => {
    setError(null);
    const result = await invoke<void>("end_session", { id });
    if (result.ok) return true;
    setError(result.error);
    return false;
  }, []);

  return { tauri, active, loading, error, createSession, endSession, refreshActive };
}
