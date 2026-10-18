import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import * as api from "../bridge/api";
import { STEALTH_CHANGED_EVENT } from "../bridge/eventNames";
import { onEvent } from "../bridge/events";
import { backendAvailable } from "../bridge/runtime";
import { Icon } from "./Icon";

const StealthContext = createContext({ enabled: false, refresh: async () => {} });
export const useStealth = () => useContext(StealthContext);

/** Reads the saved protection state in both windows; never claims protection before it is known. */
export function StealthProvider({ children }: { children: ReactNode }) {
  const [enabled, setEnabled] = useState(false);
  const refresh = useCallback(async () => {
    if (!backendAvailable()) return;
    const result = await api.stealth.get();
    if (result.ok) setEnabled(result.value);
  }, []);

  useEffect(() => {
    let cancelled = false;
    // Register before reading, so a change in the other window is not missed.
    const stop = onEvent<boolean>(STEALTH_CHANGED_EVENT, setEnabled);
    if (backendAvailable()) void api.stealth.get().then((result) => {
      if (!cancelled && result.ok) setEnabled(result.value);
    });
    window.addEventListener("focus", refresh);
    return () => {
      cancelled = true;
      stop();
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);

  return (
    <StealthContext.Provider value={{ enabled, refresh }}>
      {children}
    </StealthContext.Provider>
  );
}

export function StealthStatus({ compact = false }: { compact?: boolean }) {
  const { enabled } = useStealth();
  if (!enabled) return null;
  return <span className="stealth-status" data-compact={compact || undefined} role="status"
    aria-label="Stealth on. Hidden from screen sharing." title="Stealth on · Hidden from screen sharing">
    <Icon name="shield" size={14} />
    {!compact ? <><b>Stealth</b><span>Hidden from screen sharing</span></> : null}
  </span>;
}
