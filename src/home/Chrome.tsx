import { useEffect, useState } from "react";
import * as api from "../bridge/api";
import { onEvent } from "../bridge/events";
import { QUIT_REQUESTED_EVENT } from "../bridge/eventNames";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";

/**
 * The window uses the Mac's own red/yellow/green buttons now. What is left
 * here is the "Quit Aside?" question, asked for ⌘Q from anywhere.
 */
export function QuitGuard() {
  const [asking, setAsking] = useState(false);

  useEffect(() => onEvent(QUIT_REQUESTED_EVENT, () => setAsking(true)), []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "q") {
        event.preventDefault();
        setAsking(true);
        void api.windows.requestQuit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <Dialog
      open={asking}
      onClose={() => setAsking(false)}
      title="Quit Aside?"
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={() => setAsking(false)}>Cancel</Button>
          <Button variant="danger" onClick={() => void api.windows.confirmQuit()}>Quit</Button>
        </>
      }
    >
      <p>A call in progress will end.</p>
    </Dialog>
  );
}
