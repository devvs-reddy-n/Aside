/**
 * How the person likes a call to begin. Remembered on this Mac and shared by
 * both windows (they live at the same address, so they read the same note).
 */
export interface LaunchPrefs {
  /** Show only questions and answers in the live window, hiding the running transcript. */
  answersOnly: boolean;
}

export const DEFAULT_LAUNCH: LaunchPrefs = { answersOnly: false };

const KEY = "aside-launch-v1";

/** Reads whatever was saved and repairs anything odd, so a bad note never breaks launch. */
export function parseLaunchPrefs(raw: string | null): LaunchPrefs {
  if (!raw) return DEFAULT_LAUNCH;
  try {
    const value = JSON.parse(raw) as Partial<LaunchPrefs> | null;
    return { answersOnly: value?.answersOnly === true };
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
