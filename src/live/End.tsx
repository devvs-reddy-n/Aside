import { useEffect, useRef, useState } from "react";
import { Icon } from "../kit/Icon";
import { useController, useLiveUi } from "./context";

const CONFIRM_MS = 4000;

/**
 * Ending a call takes two taps, in place: the first arms the button (a bar
 * drains over four seconds), the second confirms. Esc or a tap elsewhere
 * cancels. There is no pop-up, so nothing can appear off-screen.
 */
export function End({ compact }: { compact?: boolean }) {
  const controller = useController();
  const ending = useLiveUi((state) => state.ending);
  const [armed, setArmed] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);

  const disarm = () => {
    window.clearTimeout(timer.current);
    setArmed(false);
  };

  function press() {
    if (ending) return;
    if (!armed) {
      setArmed(true);
      timer.current = window.setTimeout(() => setArmed(false), CONFIRM_MS);
      return;
    }
    disarm();
    void controller.end();
  }

  useEffect(() => {
    if (!armed) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        disarm();
      }
    };
    const onPointer = (event: PointerEvent) => {
      if (!button.current?.contains(event.target as Node)) disarm();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onPointer, true);
    };
  }, [armed]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const label = ending ? "Ending…" : armed ? (compact ? "Confirm" : "Tap to confirm") : "End";
  return (
    <button
      ref={button}
      type="button"
      className="lv-end"
      data-armed={armed || undefined}
      disabled={ending}
      onClick={press}
      aria-label={armed ? "Confirm: end session" : "End session"}
      title={armed ? "Press Esc to keep going" : "End session"}
    >
      {armed ? <i className="lv-end__fill" aria-hidden="true" /> : null}
      <Icon name={armed ? "check" : "stop"} size={14} />
      <span>{label}</span>
    </button>
  );
}
