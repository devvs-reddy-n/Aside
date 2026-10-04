/**
 * How the person likes a call to begin. Remembered on this Mac and shared by
 * both windows (they live at the same address, so they read the same note).
 */
export type LiveLayout = "focus" | "thread" | "prompter";

export const LIVE_LAYOUTS: readonly LiveLayout[] = ["focus", "thread", "prompter"];

export interface LaunchPrefs {
  /** Show only questions and answers in the live window, hiding the running transcript. */
  answersOnly: boolean;
  /** How the live window is arranged: question-and-answer stack, one conversation thread, or large reading text. */
  layout: LiveLayout;
}

export const DEFAULT_LAUNCH: LaunchPrefs = { answersOnly: true, layout: "focus" };

const KEY = "aside-launch-v1";

/** Reads whatever was saved and repairs anything odd, so a bad note never breaks launch. */
export function parseLaunchPrefs(raw: string | null): LaunchPrefs {
  if (!raw) return DEFAULT_LAUNCH;
  try {
    const value = JSON.parse(raw) as Partial<LaunchPrefs> | null;
    return {
      answersOnly: typeof value?.answersOnly === "boolean" ? value.answersOnly : DEFAULT_LAUNCH.answersOnly,
      layout: LIVE_LAYOUTS.includes(value?.layout as LiveLayout) ? (value?.layout as LiveLayout) : DEFAULT_LAUNCH.layout,
    };
  } catch {
    return DEFAULT_LAUNCH;
  }
}

export function loadLaunchPrefs(): LaunchPrefs {
  try {
    return parseLaunchPrefs(window.localStorage.getItem(KEY));
  } catch {
    return DEFAULT_LAUNCH;
  }
}

export function saveLaunchPrefs(patch: Partial<LaunchPrefs>): LaunchPrefs {
  const next = { ...loadLaunchPrefs(), ...patch };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // Not being able to remember a preference is fine; the call still works.
  }
  return next;
}
