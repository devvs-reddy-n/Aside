import { toast } from "./toast";

/**
 * Delete-with-undo. The item disappears from the screen straight away and a
 * toast offers Undo for eight seconds; only when that passes (or the toast
 * is dismissed) is the real delete run. If the app closes first, nothing was
 * deleted — the safe way round.
 */
export function deleteWithUndo(options: {
  label: string;
  /** Take the item off the screen now. */
  hide: () => void;
  /** Put it back (Undo). */
  restore: () => void;
  /** The real, permanent delete. Returns whether it worked. */
  commit: () => Promise<boolean>;
  /** Called if the real delete failed, after the item is restored. */
  onFailed?: (message: string) => void;
}): void {
  options.hide();
  let settled = false;
  const pending = () => {
    if (settled) return;
    settled = true;
    void options.commit().then((ok) => {
      if (!ok) {
        options.restore();
        options.onFailed?.(`Couldn't delete ${options.label}.`);
      }
    });
  };
  window.addEventListener("pagehide", pending, { once: true });
  toast(`Deleted ${options.label}`, {
    tone: "info",
    action: {
      label: "Undo",
      run: () => {
        settled = true;
        options.restore();
      },
    },
    onDone: pending,
  });
}
