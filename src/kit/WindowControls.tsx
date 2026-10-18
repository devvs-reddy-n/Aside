import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "../bridge/invoke";
import { isTauriRuntime } from "../bridge/runtime";
import { isMac } from "../core/platform";
import { windows } from "../bridge/api";
import { useStealth } from "./Stealth";
import { BrandMark } from "./BrandMark";

/** Standard hide/quit keys apply in the workspace, live window and bubble. */
export function WindowKeys() {
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const key = (event: KeyboardEvent) => {
      if (!(isMac ? event.metaKey : event.ctrlKey) || event.shiftKey || event.altKey) return;
      const letter = event.key.toLowerCase();
      if (letter === "h") { event.preventDefault(); void invoke("toggle_visibility"); }
      if (letter === "q") { event.preventDefault(); void windows.requestQuit(); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  return null;
}

/** Drag anywhere on the bubble; a click restores the saved windows. */
export function MinimizedBubble() {
  const { enabled } = useStealth();
  const down = useRef<{ x: number; y: number; dragged: boolean } | null>(null);
  return <div className="aside-bubble-wrap"><button type="button" className="aside-bubble" data-stealth={enabled || undefined} aria-label="Restore Aside" title="Drag to move · Click to restore Aside"
    onPointerDown={(event) => { if (event.button === 0) { down.current = { x: event.screenX, y: event.screenY, dragged: false }; event.currentTarget.setPointerCapture(event.pointerId); } }}
    onPointerMove={(event) => { const point = down.current; if (point && !point.dragged && Math.hypot(event.screenX - point.x, event.screenY - point.y) > 4) { point.dragged = true; if (isTauriRuntime()) void getCurrentWindow().startDragging(); } }}
    onClick={(event) => { if (event.detail === 0 || !down.current?.dragged) void invoke("restore_bubble"); down.current = null; }}>
    <BrandMark size={30} />
  </button></div>;
}
