import { useCallback, useEffect, useState } from "react";

export type ThemeMode = "light" | "dark";

const STORAGE_KEY = "aside-theme";

function systemPreference(): ThemeMode {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function readStored(): ThemeMode | null {
  const raw = window.localStorage.getItem(STORAGE_KEY);
  return raw === "light" || raw === "dark" ? raw : null;
}

/**
 * Stamps `data-theme` on `<html>` immediately, before React mounts, so the
 * main window never flashes the wrong colours on load. Call once from
 * `main.tsx`. The live overlay ignores this attribute entirely — it always
 * stays dark for its own reason (glare during a live call), unrelated to
 * this preference.
 */
export function applyStoredTheme(): void {
  document.documentElement.dataset.theme = readStored() ?? systemPreference();
}

export interface ThemeState {
  mode: ThemeMode;
  setMode: (next: ThemeMode) => void;
  toggle: () => void;
}

/**
 * Light/dark preference for the main window. A per-device display setting,
 * not app data, so it's kept in `localStorage` rather than round-tripping
 * through the Rust backend the way other Settings toggles do.
 */
export function useTheme(): ThemeState {
  const [mode, setModeState] = useState<ThemeMode>(() => readStored() ?? systemPreference());

  useEffect(() => {
    document.documentElement.dataset.theme = mode;
  }, [mode]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
    window.localStorage.setItem(STORAGE_KEY, next);
  }, []);

  const toggle = useCallback(() => {
    setMode(mode === "dark" ? "light" : "dark");
  }, [mode, setMode]);

  return { mode, setMode, toggle };
}
