import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { isDemoMode, isTauriRuntime, NOT_TAURI_MESSAGE } from "./runtime";

/** The result of a call to the Rust backend. Never throws: every caller
 *  gets back a plain object and decides what to show. */
export type InvokeResult<T> = { ok: true; value: T } | { ok: false; error: string };

/**
 * Calls one Rust command by name and returns its result without ever
 * throwing. Every backend command in `src-tauri/src/lib.rs`'s
 * `generate_handler!` list can be called through this.
 *
 * `args` must use the exact camelCase names Tauri expects on the JavaScript
 * side (Tauri turns a Rust `snake_case` parameter into `camelCase`
 * automatically) — passing the Rust name instead fails silently at
 * runtime with no compile-time warning, so every call site in this project
 * double-checks the real command signature before writing the call.
 */
export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<InvokeResult<T>> {
  if (!isTauriRuntime()) {
    if (isDemoMode()) {
      const { mockInvoke } = await import("./mock");
      return mockInvoke<T>(command, args);
    }
    return { ok: false, error: NOT_TAURI_MESSAGE };
  }
  try {
    const value = await tauriInvoke<T>(command, args);
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: describeInvokeError(error) };
  }
}

/** Rust command errors usually arrive as a plain string (every command in
 *  this project returns `Result<T, String>`). Anything else still gets
 *  turned into readable text rather than `[object Object]`. */
function describeInvokeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}
