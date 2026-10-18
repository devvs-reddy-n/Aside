import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauriRuntime } from "./runtime";
import { privacySettingsUrl } from "../core/platform";

export type PrivacyPane = "microphone" | "screen";

/** Opens the matching audio/privacy page on this computer. */
export async function openPrivacySettings(pane: PrivacyPane): Promise<void> {
  if (!isTauriRuntime()) return;
  const url = privacySettingsUrl(pane);
  if (url) await openUrl(url).catch(() => undefined);
}
