import { useEffect, useRef, useState } from "react";
import { createAutosaver, type Autosaver, type SaveState } from "../core/autosave";

/**
 * Saves `value` 600 ms after it last changed; returns what is happening, plus a way to save now.
 * Only real edits count: whatever arrives while `enabled` is false (or the moment it turns on) is
 * treated as loaded data, and a new value equal to the previous one (say, a reload of the same data)
 * does not count as a change.
 */
export function useAutosave<T>(value: T, save: (value: T) => Promise<boolean>, options: { enabled: boolean; delayMs?: number }) {
  const [state, setState] = useState<SaveState>("idle");
  const saveRef = useRef(save);
  saveRef.current = save;
  const lastSeen = useRef<string | null>(null);
  const wasEnabled = useRef(false);
  const saver = useRef<Autosaver<T> | null>(null);
  if (!saver.current) {
    saver.current = createAutosaver<T>((next) => saveRef.current(next), setState, options.delayMs ?? 600);
  }

  useEffect(() => {
    const key = JSON.stringify(value);
    const turnedOn = options.enabled && !wasEnabled.current;
    wasEnabled.current = options.enabled;
    const changed = lastSeen.current !== null && key !== lastSeen.current;
    lastSeen.current = key;
    if (options.enabled && !turnedOn && changed) saver.current?.change(value);
  }, [value, options.enabled]);

  // Leaving the page saves anything still waiting.
  useEffect(() => () => void saver.current?.flush().finally(() => saver.current?.dispose()), []);

  return { state, flush: () => saver.current?.flush() ?? Promise.resolve() };
}
