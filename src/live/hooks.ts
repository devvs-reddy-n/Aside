import { useEffect, useState } from "react";
import { formatClock } from "../core/format";
import { useController } from "./context";

/** The call's running clock, redrawn every second. */
export function useElapsed(): string {
  const controller = useController();
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);
  return formatClock(Date.now() - controller.live.get().startedAtMs);
}

/**
 * Tauri turns a double-click on a drag area into "maximize". For this small
 * always-on-top window that would throw it across the whole screen, so the
 * second click is swallowed before Tauri sees it. Single drags still work.
 */
export const noMaximize = {
  onMouseDownCapture: (event: React.MouseEvent) => {
    if (event.detail >= 2) event.stopPropagation();
  },
};
