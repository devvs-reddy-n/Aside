/**
 * Saves a value a short while after the last change, so typing never saves on
 * every keystroke, and reports what it is doing ("Saving…", "Saved", failed).
 * Framework-free so it can be tested with fake timers; `kit/useAutosave`
 * wraps it for screens.
 */
export type SaveState = "idle" | "pending" | "saving" | "saved" | "error";

export interface Autosaver<T> {
  /** Note a new value; saves `delayMs` after the last call. */
  change: (value: T) => void;
  /** Save now if something is waiting (e.g. before leaving the page). */
  flush: () => Promise<boolean>;
  dispose: () => void;
}

export function createAutosaver<T>(
  save: (value: T) => Promise<boolean>,
  onState: (state: SaveState) => void,
  delayMs = 600,
): Autosaver<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let waiting: { value: T } | null = null;
  let running: Promise<boolean> | null = null;
  let failed: { value: T } | null = null;
  let lastOk = true;

  const hasWork = () => waiting !== null || running !== null;

  const run = async (): Promise<boolean> => {
    // A save may already have consumed its queue but still be writing.
    if (running) {
      await running;
      return hasWork() ? run() : lastOk;
    }
    if (!waiting) return lastOk;
    const current = waiting;
    waiting = null;
    failed = null;
    onState("saving");
    running = Promise.resolve().then(() => save(current.value))
      .catch(() => false)
      .then((ok) => {
        lastOk = ok;
        if (!ok && !waiting) failed = current;
        onState(waiting ? "pending" : ok ? "saved" : "error");
        return ok;
      })
      .finally(() => { running = null; });
    await running;
    return hasWork() ? run() : lastOk;
  };

  return {
    change(value) {
      waiting = { value };
      failed = null;
      onState("pending");
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void run(), delayMs);
    },
    async flush() {
      if (timer) clearTimeout(timer);
      if (!running && !waiting && failed) {
        waiting = failed;
        failed = null;
      }
      return run();
    },
    dispose() {
      if (timer) clearTimeout(timer);
    },
  };
}
