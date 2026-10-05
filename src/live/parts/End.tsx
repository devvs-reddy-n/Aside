import { useEffect, useState } from "react";
import { Icon } from "../../kit/Icon";
import { useController, useLiveUi } from "../context";

const CANCEL_MS = 4000;

/** One click schedules the end; the red countdown is the chance to cancel (click again or Escape). */
export function End() {
  const controller = useController();
  const ending = useLiveUi((state) => state.ending);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!pending) return;
    const timer = window.setTimeout(() => {
      setPending(false);
      void controller.end();
    }, CANCEL_MS);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setPending(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [pending, controller]);

  const label = ending ? "Ending…" : pending ? "Cancel" : "End";
  return (
    <button
      type="button"
      className="lv-end"
      data-armed={pending || undefined}
      disabled={ending}
      onClick={() => setPending((value) => !value)}
      aria-label={pending ? "Cancel ending session" : "End session"}
      title={pending ? "Ending in four seconds. Click or press Escape to cancel." : "End session after a four-second cancel window"}
    >
      {pending ? <i className="lv-end__fill" aria-hidden="true" /> : null}
      <Icon name={pending ? "x" : "stop"} size={13} />
      <span>{label}</span>
    </button>
  );
}
