import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import { WINDOW_MEMORY_EVENT } from "../platform/eventNames";
import type { PressLog, WindowPreview } from "../platform/types";

export interface WindowMemoryState {
  tauri: boolean;
  /** A small, fixed set of DB-backed facts (visa status, notice period,
   *  ...) — not window summaries, never grows with session length. */
  facts: string[];
  /** One 1-2 sentence plain-English summary per completed conversation
   *  window, oldest first, appended for the whole session with no cap.
   *  Produced asynchronously after a Command+Enter press, from the real
   *  transcript only — never from Aside's own suggested answer. */
  summary: string[];
  /** What has been said since the last Command+Enter — this is exactly
   *  what would be sent if the owner pressed it right now. */
  rawWindow: string;
  /** How many times Command+Enter has been pressed this session. */
  pressCount: number;
  /** The most recent Command+Enter's own diagnostics (token estimates,
   *  timing) — `null` before the first press this session. */
  lastLog: PressLog | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

/**
 * What Aside remembers from the conversation right now: the known facts,
 * the running per-window summary, and the window of speech waiting for the
 * next press. `get_window_preview` is the source of truth; the
 * `window-memory` event fires both after a Command+Enter press and
 * whenever the background summarizer finishes a window (so `summary` can
 * change well after the press that produced the answer), but its own
 * `raw_window` field is deliberately left blank in the press case (the
 * window that was just answered is already gone), so every event triggers
 * a fresh `get_window_preview` read rather than trusting the event payload
 * directly — see `WINDOW_MEMORY_EVENT`'s doc comment.
 */
export function useWindowMemory(): WindowMemoryState {
  const tauri = isTauriRuntime();
  const [facts, setFacts] = useState<string[]>([]);
  const [summary, setSummary] = useState<string[]>([]);
  const [rawWindow, setRawWindow] = useState("");
  const [pressCount, setPressCount] = useState(0);
  const [lastLog, setLastLog] = useState<PressLog | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setLoading(true);
    const result = await invoke<WindowPreview>("get_window_preview");
    if (result.ok) {
      setFacts(result.value.facts);
      setSummary(result.value.summary);
      setRawWindow(result.value.raw_window);
      setPressCount(result.value.press_count);
      setLastLog(result.value.last_log);
      setError(null);
    } else {
      setError(result.error);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useTauriEvent<WindowPreview>(WINDOW_MEMORY_EVENT, () => {
    void refresh();
  });

  return { tauri, facts, summary, rawWindow, pressCount, lastLog, loading, error, refresh };
}
