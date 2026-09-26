import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauriRuntime } from "./runtime";

/**
 * Listens to one event from the Rust side and hands back a plain function
 * that stops listening. Safe to call and cancel straight away: if the
 * cancel comes before the listener has finished registering, it is
 * dropped the moment it does. A no-op in a plain browser tab.
 */
export function onEvent<T>(name: string, handler: (payload: T) => void): () => void {
  if (!isTauriRuntime()) return () => undefined;
  let stop: UnlistenFn | null = null;
  let cancelled = false;
  listen<T>(name, (event) => handler(event.payload))
    .then((fn) => {
      if (cancelled) fn();
      else stop = fn;
    })
    .catch(() => undefined);
  return () => {
    cancelled = true;
    if (stop) stop();
  };
}

/** Several `onEvent` subscriptions as one cancel function. */
export function onEvents(subscriptions: Array<() => void>): () => void {
  return () => subscriptions.forEach((stop) => stop());
}
