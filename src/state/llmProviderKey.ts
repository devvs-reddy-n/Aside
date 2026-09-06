import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type { LlmProviderKind } from "../platform/types";

export interface LlmProviderKeyState {
  tauri: boolean;
  /** Whether a key is already saved for this provider. Never exposes the
   *  key itself once saved — the backend deliberately never returns it,
   *  see `commands::llm::has_llm_api_key`'s doc comment. */
  hasKey: boolean;
  saving: boolean;
  error: string | null;
  saveKey: (apiKey: string) => Promise<boolean>;
  refresh: () => Promise<void>;
}

/**
 * Whether an API key is saved for one AI answer provider, and the action
 * to save one. Used by Setup's "OpenAI key" step today; Settings (layer 7)
 * reuses this for every provider it lists later.
 */
export function useLlmProviderKey(provider: LlmProviderKind): LlmProviderKeyState {
  const tauri = isTauriRuntime();
  const [hasKey, setHasKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const result = await invoke<boolean>("has_llm_api_key", { provider });
    if (result.ok) setHasKey(result.value);
    else setError(result.error);
  }, [provider]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveKey = useCallback(
    async (apiKey: string) => {
      setSaving(true);
      setError(null);
      const result = await invoke<void>("set_llm_api_key", { provider, apiKey });
      setSaving(false);
      if (!result.ok) {
        setError(result.error);
        return false;
      }
      setHasKey(true);
      return true;
    },
    [provider],
  );

  return { tauri, hasKey, saving, error, saveKey, refresh };
}
