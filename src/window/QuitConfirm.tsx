import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Dialog, Button } from "../ui";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { QUIT_REQUESTED_EVENT } from "../platform/eventNames";

/** ⌘Q in this webview asks to quit. The live overlay uses this alone so
 *  its own window never shows the workspace dialog. */
export function useQuitShortcut(): void {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || event.shiftKey || event.altKey) return;
      if (event.key.toLowerCase() !== "q") return;
      event.preventDefault();
      void invoke("request_quit");
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/**
 * Quit confirmation used by the title-bar cross, ⌘Q, and Aside > Quit.
 * Opening always goes through the Rust `request_quit` command so hidden
 * windows are brought back before the dialog appears.
 */
export function useQuitConfirm(): {
  dialog: ReactNode;
  askToQuit: () => void;
} {
  const [open, setOpen] = useState(false);
  useQuitShortcut();
  useTauriEvent(QUIT_REQUESTED_EVENT, () => setOpen(true));

  const askToQuit = useCallback(() => {
    setOpen(true);
    void invoke("request_quit");
  }, []);

  const dialog = (
    <Dialog
      open={open}
      onClose={() => setOpen(false)}
      title="Quit Aside?"
      destructive
      footer={
        <>
          <Button variant="secondary" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={() => void invoke("confirm_quit")}>
            Quit
          </Button>
        </>
      }
    >
      <p>This closes Aside completely. A live call will end.</p>
    </Dialog>
  );

  return { dialog, askToQuit };
}
