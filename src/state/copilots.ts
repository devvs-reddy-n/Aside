import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type { Copilot, CopilotInput, CopilotSummary, Session } from "../platform/types";

export function useCopilots() {
  const [copilots, setCopilots] = useState<CopilotSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setLoading(true);
    const result = await invoke<CopilotSummary[]>("list_copilots");
    if (result.ok) { setCopilots(result.value); setError(null); } else setError(result.error);
    setLoading(false);
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const save = useCallback(async (id: string | null, input: CopilotInput): Promise<Copilot | null> => {
    setError(null);
    const result = id
      ? await invoke<Copilot>("update_copilot", { copilotId: id, input })
      : await invoke<Copilot>("create_copilot", { input });
    if (!result.ok) { setError(result.error); return null; }
    await refresh();
    return result.value;
  }, [refresh]);

  const start = useCallback(async (id: string): Promise<Session | null> => {
    setError(null);
    const result = await invoke<{ session: Session }>("start_copilot_call", { copilotId: id });
    if (!result.ok) { setError(result.error); return null; }
    return result.value.session;
  }, []);

  const remove = useCallback(async (id: string) => {
    const result = await invoke<boolean>("delete_copilot", { copilotId: id });
    if (!result.ok) { setError(result.error); return false; }
    await refresh(); return result.value;
  }, [refresh]);

  return { copilots, loading, error, refresh, save, start, remove };
}
