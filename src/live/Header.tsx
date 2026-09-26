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
  silent: { tone: "warn", text: "Can't hear others" },
  paused: { tone: "off", text: "Paused" },
  ended: { tone: "off", text: "Ended" },
};

export function usePhase() {
  const phase = useLive(derivePhase);
  return { phase, ...PHASES[phase] };
}

export function StatusPill() {
  const { tone, text } = usePhase();
  return (
    <span className="lv-pill" data-tone={tone} role="status">
      <i aria-hidden="true" />
      <em>{text}</em>
    </span>
  );
}

export function Header({ room }: { room: Room }) {
  const session = useLive((state) => state.session);
  const elapsed = useElapsed();
  const { tone } = usePhase();
  const [menuOpen, setMenuOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  /** Under 480px wide the status pill and clock make way for the title; a dot on the logo carries the status. */
  const narrow = room.w < 480;

  const title = session?.company?.trim() || "Live session";
  const kind = session?.session_type === "interview" ? "Interview" : "Call";

  return (
    <header className="lv-head" data-tauri-drag-region {...noMaximize}>
      <span className="lv-mark" data-tone={tone} data-tauri-drag-region>
        <Icon name="mark" size={16} />
        {narrow ? <i aria-hidden="true" /> : null}
      </span>
      <div className="lv-title" data-tauri-drag-region>
        <b data-tauri-drag-region>{title}</b>
        <span data-tauri-drag-region>{narrow ? elapsed : kind}</span>
      </div>
      {narrow ? null : (
        <>
          <StatusPill />
          <span className="lv-clock" aria-label="Time on this call" data-tauri-drag-region>{elapsed}</span>
        </>
      )}
      <button ref={menuButton} type="button" className="lv-icon" aria-label="Window options" aria-haspopup="dialog" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>
        <Icon name="more" size={18} />
      </button>
      <End compact={narrow} />
      {menuOpen ? <Menu anchor={menuButton} onClose={() => setMenuOpen(false)} /> : null}
    </header>
  );
}
