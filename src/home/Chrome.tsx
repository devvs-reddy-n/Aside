import { useEffect, useState } from "react";
import * as api from "../bridge/api";
import { onEvent } from "../bridge/events";
import { QUIT_REQUESTED_EVENT } from "../bridge/eventNames";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";

/**
 * The window uses the Mac's own red/yellow/green buttons now. What is left
 * here is the "Quit Aside?" question, opened by the app menu's Quit. The red
 * window button and keyboard Quit use the same confirmation.
 */
export function QuitGuard() {
  const [asking, setAsking] = useState(false);

  useEffect(() => onEvent(QUIT_REQUESTED_EVENT, () => setAsking(true)), []);

  return (
    <Dialog
      open={asking}
      onClose={() => setAsking(false)}
      title="Quit Aside?"
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={() => setAsking(false)}>Cancel</Button>
          <Button className="btn--end-quit" variant="danger" onClick={() => void api.windows.confirmQuit()}>Quit</Button>
        </>
      }
    >
      <p>A call in progress will end.</p>
    </Dialog>
  );
}
