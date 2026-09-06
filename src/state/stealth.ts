import { useCallback, useEffect, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import { STEALTH_CHANGED_EVENT } from "../platform/eventNames";

export interface StealthState {
  /** True when Aside is left out of screenshots, recordings, and meeting
   *  screen-shares. Default is on. */
  enabled: boolean;
  toggle: () => Promise<void>;
  setEnabled: (next: boolean) => Promise<void>;
}

/**
 * Shared screen-share hiding flag. Settings is the only on/off. Changing
 * it saves the flag and relaunches Aside so Dock and menu-bar policy apply
 * from the first frame. The `stealth-changed` event is still listened for
 * so a future in-process update would stay in sync.
 */
export function useStealth(): StealthState {
  const [enabled, setEnabledState] = useState(true);

  useEffect(() => {
    void invoke<boolean>("get_stealth").then((result) => {
      if (result.ok) setEnabledState(result.value);
    });
  }, []);

  useTauriEvent<boolean>(STEALTH_CHANGED_EVENT, (next) => {
    setEnabledState(next);
  });

  const setEnabled = useCallback(async (next: boolean) => {
    setEnabledState(next);
    const result = await invoke<boolean>("set_stealth", { enabled: next });
    if (result.ok) {
      setEnabledState(result.value);
    } else if (isTauriRuntime()) {
      setEnabledState(!next);
    }
  }, []);

  const toggle = useCallback(async () => {
    await setEnabled(!enabled);
  }, [enabled, setEnabled]);

  return { enabled, toggle, setEnabled };
}
