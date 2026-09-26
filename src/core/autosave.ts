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
  flush: () => Promise<void>;
  dispose: () => void;
}

export function createAutosaver<T>(
  save: (value: T) => Promise<boolean>,
  onState: (state: SaveState) => void,
  delayMs = 600,
): Autosaver<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let waiting: { value: T } | null = null;
  let running: Promise<void> | null = null;

  const run = async (): Promise<void> => {
    if (!waiting) return;
    if (running) {
      await running;
      return run();
    }
    const { value } = waiting;
    waiting = null;
    onState("saving");
    running = save(value)
      .then((ok) => onState(ok ? (waiting ? "pending" : "saved") : "error"))
      .catch(() => onState("error"))
      .finally(() => {
        running = null;
      });
    await running;
    if (waiting) await run();
  };

  return {
    change(value) {
      waiting = { value };
      onState("pending");
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void run(), delayMs);
    },
    async flush() {
      if (timer) clearTimeout(timer);
      await run();
    },
    dispose() {
      if (timer) clearTimeout(timer);
    },
  };
}
