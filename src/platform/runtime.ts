/**
 * Whether this code is running inside the real Aside desktop app (a Tauri
 * webview) versus a plain browser tab (e.g. `npm run dev` opened in Chrome
 * for a quick look at layout).
 *
 * This matters everywhere: a browser tab has no microphone access to the
 * app's audio devices, no file system, and no Rust backend behind it, so
 * every layer must check this and show a plain explanation instead of
 * silently doing nothing (which would just look broken).
 */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Plain-English message every layer can show when it detects a browser
 *  preview instead of the real app. */
export const NOT_TAURI_MESSAGE =
  "This screen needs the real Aside app to work. You are looking at a plain web preview right now, which cannot reach your microphone, your speakers, or your saved files. Open Aside.app instead.";
