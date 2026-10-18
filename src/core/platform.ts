export type DesktopPlatform = "macos" | "windows" | "other";

export function platformFrom(userAgent: string): DesktopPlatform {
  if (/Windows|Win32|Win64/i.test(userAgent)) return "windows";
  if (/Macintosh|MacIntel|MacPPC|Mac OS X/i.test(userAgent)) return "macos";
  return "other";
}

export const desktopPlatform = platformFrom(typeof navigator === "undefined" ? "" : navigator.userAgent);
export const isMac = desktopPlatform === "macos";
export const systemSettingsLabel = isMac ? "Open System Settings" : "Open Windows Settings";

export function privacySettingsUrl(pane: "microphone" | "screen", platform = desktopPlatform): string | null {
  if (platform === "windows") return pane === "microphone" ? "ms-settings:privacy-microphone" : "ms-settings:sound";
  if (platform === "macos") return `x-apple.systempreferences:com.apple.preference.security?Privacy_${pane === "microphone" ? "Microphone" : "ScreenCapture"}`;
  return null;
}

export function audioPermissionHelp(channel: "mic" | "system", platform = desktopPlatform): string {
  if (platform === "windows") return channel === "mic"
    ? "In Windows Settings → Privacy & security → Microphone, allow microphone access for desktop apps, then try again."
    : "In Windows Settings → System → Sound, check the output device. Play sound through it, then try again.";
  return channel === "mic"
    ? "Allow Aside in System Settings → Privacy & Security → Microphone, then try again."
    : "Allow Aside under Screen & System Audio Recording, then check again.";
}

export function screenCaptureHelp(platform = desktopPlatform): string {
  return platform === "windows"
    ? "Check that your display is connected and unlocked, then try again. Windows can block capture of protected content."
    : "Enable Aside in System Settings → Privacy & Security → Screen & System Audio Recording, then reopen Aside.";
}
