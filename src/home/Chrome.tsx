import { useEffect, useState } from "react";
import * as api from "../bridge/api";
import { onEvent } from "../bridge/events";
import { QUIT_REQUESTED_EVENT } from "../bridge/eventNames";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Icon } from "../kit/Icon";

/**
 * The strip at the top of the window. The window has no system title bar, so
 * this is what you drag by, and it carries the two things a title bar
 * normally does: hide (same as ⌘⇧H) and quit (asks first, same as ⌘Q).
 */
export function Chrome() {
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
    <div className="hm-chrome" data-tauri-drag-region>
      <span className="hm-chrome__brand" data-tauri-drag-region>
        <Icon name="mark" size={15} />
        <b data-tauri-drag-region>Aside</b>
      </span>
      <span className="grow" data-tauri-drag-region />
      <button type="button" className="icon-btn icon-btn--sm" aria-label="Hide Aside" title="Hide from your screen (⌘⇧H)" onClick={() => void api.windows.toggleOnScreen()}>
        <Icon name="minus" size={16} />
      </button>
      <button
        type="button"
        className="icon-btn icon-btn--sm"
        aria-label="Quit Aside"
        title="Quit (⌘Q)"
        onClick={() => {
          setAsking(true);
          void api.windows.requestQuit();
        }}
      >
        <Icon name="x" size={16} />
      </button>
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
        <p>This closes Aside completely. A call in progress will end.</p>
      </Dialog>
    </div>
  );
}
