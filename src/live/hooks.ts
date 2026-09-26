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

/** How much room the window has. The layout picks a shape from these, not from CSS container queries. */
export interface Room {
  width: "narrow" | "regular" | "wide";
  height: "tiny" | "short" | "regular";
  w: number;
  h: number;
}

export function roomFor(w: number, h: number): Room {
  return {
    w,
    h,
    width: w < 390 ? "narrow" : w >= 560 ? "wide" : "regular",
    height: h < 330 ? "tiny" : h < 460 ? "short" : "regular",
  };
}

/** Follows the window's real inner size (and keeps a CSS height variable in step with it). */
export function useRoom(): Room {
  const [room, setRoom] = useState<Room>(() => roomFor(window.innerWidth, window.innerHeight));
  useEffect(() => {
    const read = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      document.documentElement.style.setProperty("--lv-h", `${h}px`);
      document.documentElement.style.setProperty("--lv-w", `${w}px`);
      setRoom((current) => (current.w === w && current.h === h ? current : roomFor(w, h)));
    };
    read();
    window.addEventListener("resize", read);
    const observer = new ResizeObserver(read);
    observer.observe(document.documentElement);
    return () => {
      window.removeEventListener("resize", read);
      observer.disconnect();
    };
  }, []);
  return room;
}
