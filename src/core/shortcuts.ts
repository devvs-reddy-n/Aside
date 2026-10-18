import { desktopPlatform, type DesktopPlatform } from "./platform.ts";

/** Matches Rust's CmdOrCtrl registration, including Shift+Comma/Period. */
export function shortcutsFor(platform: DesktopPlatform) {
  return platform === "macos"
    ? [["⌘ ↩", "Answer"], ["⌘ ⇧ ↩", "Screenshot answer"], ["⌘ <", "Previous answer"], ["⌘ >", "Next answer"]] as const
    : [["Ctrl + Enter", "Answer"], ["Ctrl + Shift + Enter", "Screenshot answer"], ["Ctrl + Shift + ,", "Previous answer"], ["Ctrl + Shift + .", "Next answer"]] as const;
}

export const APP_SHORTCUTS = shortcutsFor(desktopPlatform);
export const ANSWER_SHORTCUT = APP_SHORTCUTS[0][0];
export const SCREENSHOT_SHORTCUT = APP_SHORTCUTS[1][0];
export const PREVIOUS_SHORTCUT = APP_SHORTCUTS[2][0];
export const NEXT_SHORTCUT = APP_SHORTCUTS[3][0];
