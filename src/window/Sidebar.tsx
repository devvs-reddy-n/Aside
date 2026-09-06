import { useEffect, useState } from "react";
import { Icon, type IconName } from "../ui/Icon";
import { StatusOrb } from "../ui/StatusOrb";
import { useSttEngine, engineNeedsKey } from "../state/sttEngine";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type { LlmProviderKind } from "../platform/types";

export type NavDestination = "copilots" | "sessions" | "resume" | "documents";

interface NavItem {
  id: NavDestination;
  label: string;
  icon: IconName;
}

/* Two checklist groups instead of one flat list (2026-08-30, "Cockpit
 * Instrumentation"): set up the flight, then read the instruments — the
 * same two moments PRODUCT.md's "Operating Context" describes as
 * structurally different (before/after the call vs. during it). Settings
 * stays outside both groups, rendered separately below. */
const PREFLIGHT_ITEMS: NavItem[] = [
  { id: "copilots", label: "Copilots", icon: "spark" },
  { id: "resume", label: "Resumes", icon: "document" },
  { id: "documents", label: "Documents", icon: "note" },
];

const INSTRUMENT_ITEMS: NavItem[] = [{ id: "sessions", label: "History", icon: "list" }];

export interface SidebarProps {
  active: NavDestination;
  onNavigate: (destination: NavDestination) => void;
  onOpenSettings: () => void;
}

/**
 * The `main` window's left glass column (spec: "narrow translucent glass
 * sidebar" — not the old 52px icon-only spine). Active item gets a soft
 * accent wash (`--accent-dim`), never a solid fill. The bottom status rows
 * are real readiness readouts, not live capture meters — mic/system audio
 * only have live state once a call is connected (that's the overlay's
 * job), so this shows what *can* be checked from here: whether
 * speech-to-text is ready, which answer model is configured, and whether
 * the desktop app is actually reachable.
 */
export function Sidebar({ active, onNavigate, onOpenSettings }: SidebarProps) {
  const stt = useSttEngine();
  const [model, setModel] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    void (async () => {
      const provider = await invoke<LlmProviderKind | null>("get_llm_provider");
      if (!provider.ok || !provider.value) return;
      const model = await invoke<string | null>("get_llm_model", { provider: provider.value });
      if (model.ok && model.value) setModel(model.value);
    })();
  }, []);

  const sttReady =
    stt.engine != null && (!engineNeedsKey(stt.engine) || stt.hasKey) &&
    (stt.engine !== "parakeet_local" || stt.modelDownloaded);
  const connected = isTauriRuntime();

  return (
    <nav className="aw-sidebar" aria-label="Aside">
      <div className="aw-sidebar__nav">
        <div className="aw-sidebar__group">
          <p className="aw-sidebar__group-label">Workspace</p>
          {PREFLIGHT_ITEMS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`aw-sidebar__item${active === item.id ? " aw-sidebar__item--active" : ""}`}
              onClick={() => onNavigate(item.id)}
              aria-current={active === item.id ? "page" : undefined}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </button>
          ))}
        </div>

        <div className="aw-sidebar__group">
          <p className="aw-sidebar__group-label">Live work</p>
          {INSTRUMENT_ITEMS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`aw-sidebar__item${active === item.id ? " aw-sidebar__item--active" : ""}`}
              onClick={() => onNavigate(item.id)}
              aria-current={active === item.id ? "page" : undefined}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </button>
          ))}
        </div>

        <button type="button" className="aw-sidebar__item aw-sidebar__item--settings" onClick={onOpenSettings}>
          <Icon name="settings" />
          <span>Settings</span>
        </button>
      </div>

      <div className="aw-sidebar__status">
        <p className="aw-sidebar__group-label">Systems</p>
        <StatusOrb tone={sttReady ? "ok" : "neutral"}>Speech-to-text {sttReady ? "ready" : "not set up"}</StatusOrb>
        <StatusOrb tone={model ? "ok" : "neutral"}>{model ? `Model: ${model}` : "No model chosen"}</StatusOrb>
        <StatusOrb tone={connected ? "ok" : "bad"}>{connected ? "Connected" : "Preview only"}</StatusOrb>
      </div>
    </nav>
  );
}
