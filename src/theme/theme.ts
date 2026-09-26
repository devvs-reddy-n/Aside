import { useEffect, useState } from "react";

export type ThemePreference = "system" | "light" | "dark";
const KEY = "aside-theme-v2";

function read(): ThemePreference {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
    // The previous design stored only light or dark under another key.
    const legacy = window.localStorage.getItem("aside-theme");
    if (legacy === "light" || legacy === "dark") return legacy;
  } catch {
    // Storage can be unavailable; the app simply follows the system.
  }
  return "system";
}

function resolve(preference: ThemePreference): "light" | "dark" {
  if (preference !== "system") return preference;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function stamp(preference: ThemePreference): void {
  document.documentElement.dataset.theme = resolve(preference);
}

/** Called once before anything draws, so a window never flashes the wrong colours. */
export function applyStoredTheme(): void {
  stamp(read());
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => stamp(read()));
}

export function useTheme(): { preference: ThemePreference; setPreference: (next: ThemePreference) => void } {
  const [preference, setPref] = useState<ThemePreference>(read);
  useEffect(() => stamp(preference), [preference]);
  const setPreference = (next: ThemePreference) => {
    setPref(next);
    try {
      window.localStorage.setItem(KEY, next);
    } catch {
      // Not remembered across launches, but still applied now.
    }
  };
  return { preference, setPreference };
}
