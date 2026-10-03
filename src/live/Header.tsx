import { useCallback, useRef, useState } from "react";
import { Icon } from "../kit/Icon";
import { useLive } from "./context";
import { AudioControl, ViewTabs } from "./Strip";
import { End } from "./End";
import { noMaximize, useElapsed } from "./hooks";
import { derivePhase, type LivePhase } from "./machine";
import { Menu } from "./Menu";

const PHASES: Record<LivePhase, { tone: "ok" | "info" | "warn" | "bad" | "off"; text: string }> = {
  listening: { tone: "ok", text: "Listening" },
  answering: { tone: "info", text: "Answering" },
  question: { tone: "info", text: "Question" },
  offline: { tone: "bad", text: "Offline" },
  silent: { tone: "warn", text: "Audio unavailable" },
  paused: { tone: "off", text: "Paused" },
  ended: { tone: "off", text: "Ended" },
};

export function usePhase() {
  const phase = useLive(derivePhase);
  return { phase, ...PHASES[phase] };
}

/** Floating mode, audio and session controls; the gaps remain draggable. */
export function Header() {
  const elapsed = useElapsed();
  const { text } = usePhase();
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);

  return (
    <header className="lv-head" data-tauri-drag-region {...noMaximize}>
      <ViewTabs />
      <span className="lv-head-spacer" data-tauri-drag-region title={`${text} · ${elapsed}`} />
      <AudioControl channel="mic" />
      <AudioControl channel="system" />
      <span className="lv-clock" aria-label={`${text}. Time on this call ${elapsed}`} data-tauri-drag-region>{elapsed}</span>
      <button ref={menuButton} type="button" className="lv-icon" aria-label="Window options" title="Window options" aria-haspopup="dialog" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>
        <Icon name="more" size={18} />
      </button>
      <End />
      {menuOpen ? <Menu anchor={menuButton} onClose={closeMenu} /> : null}
    </header>
  );
}
