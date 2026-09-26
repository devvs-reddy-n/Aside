import { useRef, useState } from "react";
import { Icon } from "../kit/Icon";
import { useLive } from "./context";
import { End } from "./End";
import { noMaximize, useElapsed, type Room } from "./hooks";
import { derivePhase, type LivePhase } from "./machine";
import { Menu } from "./Menu";

const PHASES: Record<LivePhase, { tone: "ok" | "info" | "warn" | "bad" | "off"; text: string }> = {
  listening: { tone: "ok", text: "Listening" },
  answering: { tone: "info", text: "Answering" },
  question: { tone: "info", text: "Question" },
  offline: { tone: "bad", text: "Offline" },
  silent: { tone: "warn", text: "Can’t hear them" },
  paused: { tone: "off", text: "Paused" },
  ended: { tone: "off", text: "Ended" },
};

export function usePhase() {
  const phase = useLive(derivePhase);
  return { phase, ...PHASES[phase] };
}

/**
 * One 44px row: a live dot (it only pulses while listening), the company, the call's clock,
 * window options and End. Anything other than plain listening is said in words next to the name.
 */
export function Header({ room }: { room: Room }) {
  const session = useLive((state) => state.session);
  const elapsed = useElapsed();
  const { phase, tone, text } = usePhase();
  const [menuOpen, setMenuOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const title = session?.company?.trim() || "Live session";

  return (
    <header className="lv-head" data-tauri-drag-region {...noMaximize}>
      <span className="lv-live" data-tone={tone} data-pulse={phase === "listening" || undefined} role="img" aria-label={text} title={text} data-tauri-drag-region />
      <div className="lv-title" data-tauri-drag-region>
        <b title={title} data-tauri-drag-region>{title}</b>
        {phase !== "listening" ? <em data-tone={tone} role="status">{text}</em> : null}
      </div>
      <span className="lv-clock" aria-label={`Time on this call ${elapsed}`} data-tauri-drag-region>{elapsed}</span>
      <button ref={menuButton} type="button" className="lv-icon" aria-label="Window options" title="Window options" aria-haspopup="dialog" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>
        <Icon name="more" size={18} />
      </button>
      <End compact={room.w < 420} />
      {menuOpen ? <Menu anchor={menuButton} onClose={() => setMenuOpen(false)} /> : null}
    </header>
  );
}
