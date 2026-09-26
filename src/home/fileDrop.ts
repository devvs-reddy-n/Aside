import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { isTauriRuntime } from "../bridge/runtime";

/** Lets people drop PDF or Word files onto the window. Only exists in the real app. Returns whether files are hovering. */
export function useFileDrop(onFiles: (paths: string[]) => void, enabled = true): boolean {
  const [over, setOver] = useState(false);
  const latest = useRef(onFiles);
  latest.current = onFiles;
  useEffect(() => {
    if (!enabled || !isTauriRuntime()) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") setOver(true);
        else if (payload.type === "leave") setOver(false);
        else if (payload.type === "drop") {
          setOver(false);
          latest.current(payload.paths);
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else stop = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [enabled]);
  return over;
}
