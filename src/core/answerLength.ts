/**
 * Each copilot's preferred answer length, remembered on this Mac. When a call
 * starts, the choice is handed to the live window through a one-time note.
 */
export type LengthChoice = "auto" | "small" | "medium" | "long";

const KEY = "aside-copilot-length-v1";
const NEXT = "aside-next-length";
const VALID: LengthChoice[] = ["auto", "small", "medium", "long"];

function read(): Record<string, LengthChoice> {
  try {
    const raw = JSON.parse(window.localStorage.getItem(KEY) ?? "{}") as Record<string, unknown>;
    return Object.fromEntries(Object.entries(raw).filter(([, v]) => VALID.includes(v as LengthChoice))) as Record<string, LengthChoice>;
  } catch {
    return {};
  }
}

export function lengthFor(copilotId: string): LengthChoice {
  return read()[copilotId] ?? "auto";
}

export function setLengthFor(copilotId: string, length: LengthChoice): void {
  try {
    window.localStorage.setItem(KEY, JSON.stringify({ ...read(), [copilotId]: length }));
  } catch {
    // Not remembered; the live window falls back to its own default.
  }
}

/** Called just before the live window opens. */
export function handOffLength(copilotId: string): void {
  try {
    window.localStorage.setItem(NEXT, lengthFor(copilotId));
  } catch {
    // Nothing to hand over.
  }
}

/** Read once by the live window as it starts; the note is removed so it only applies to that call. */
export function takeHandedOffLength(): LengthChoice {
  try {
    const value = window.localStorage.getItem(NEXT) as LengthChoice | null;
    window.localStorage.removeItem(NEXT);
    return value && VALID.includes(value) ? value : "auto";
  } catch {
    return "auto";
  }
}
