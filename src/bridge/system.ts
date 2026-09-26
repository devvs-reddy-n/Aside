import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauriRuntime } from "./runtime";

export type PrivacyPane = "microphone" | "screen";

const PANES: Record<PrivacyPane, string> = {
  microphone: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
  screen: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
};

/** Opens the matching page of macOS System Settings → Privacy & Security. */
export async function openPrivacySettings(pane: PrivacyPane): Promise<void> {
  if (!isTauriRuntime()) return;
  await openUrl(PANES[pane]).catch(() => undefined);
}
