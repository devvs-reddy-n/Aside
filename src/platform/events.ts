import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauriRuntime } from "./runtime";

/**
 * Subscribes to one Rust-emitted event for the lifetime of the calling
 * component, and cleans up correctly on unmount or whenever `eventName`
 * changes. A no-op outside the real app (a browser preview has nothing to
 * listen to).
 *
 * `handler` can be a fresh closure on every render (the common case for a
 * handler that reads current state) — this hook always calls the latest
 * one via a ref, so it never needs to resubscribe just because the handler
 * reference changed, and never calls a stale closure either.
 */
export function useTauriEvent<T>(eventName: string, handler: (payload: T) => void): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;

    listen<T>(eventName, (event) => {
      handlerRef.current(event.payload);
    })
      .then((fn) => {
        if (cancelled) {
          // The subscription resolved after this effect was already torn
          // down (component unmounted, or `eventName` changed) — unlisten
          // immediately instead of leaking a live listener.
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // Listening itself failed (should not happen inside a real Tauri
        // webview) — nothing to clean up, and the caller has no error
        // channel for a subscribe failure, so this is intentionally silent.
      });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [eventName]);
}
