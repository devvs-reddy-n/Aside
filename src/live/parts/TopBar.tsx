import { useCallback, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { monogram, tintHue } from "../../core/text";
import { Icon } from "../../kit/Icon";
import { useLive } from "../context";
import { noMaximize, useElapsed } from "../hooks";
import { End } from "./End";
import { Menu } from "./Menu";
import { usePhase } from "./status";

/**
 * The top edge: which copilot, what the call is doing and for how long, the
 * window menu and End. Empty space in it drags the window. `slim` keeps only
 * the status dot and clock so the Prompter has room to read.
 */
export function TopBar({ slim = false, children }: { slim?: boolean; children?: ReactNode }) {
  const elapsed = useElapsed();
  const { phase, tone, text } = usePhase();
  const company = useLive((state) => state.session?.company?.trim() || "Live session");
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);
  const pulse = phase === "listening" || phase === "answering";

  return (
    <header className="lv-top" data-slim={slim || undefined} data-tauri-drag-region {...noMaximize}>
      {slim ? null : (
        <span className="lv-top__who" data-tauri-drag-region>
          <span className="lv-top__mono" style={{ "--h": tintHue(company) } as CSSProperties} aria-hidden="true">{monogram(company)}</span>
          <b data-tauri-drag-region title={company}>{company}</b>
        </span>
      )}
      <span className="lv-status" data-tone={tone} role="status" aria-label={`${text}. Time on this call ${elapsed}`} data-tauri-drag-region>
        <i className="lv-status__dot" data-pulse={pulse || undefined} aria-hidden="true" />
        {slim ? null : <span className="lv-status__text">{text}</span>}
        <time className="tabular">{elapsed}</time>
      </span>
      {children}
      <span className="lv-grow" data-tauri-drag-region />
      <button
        ref={menuButton}
        type="button"
        className="lv-iconbtn"
        aria-label="Window options"
        title="Window options"
        aria-haspopup="dialog"
        aria-expanded={menuOpen}
        onClick={() => setMenuOpen((open) => !open)}
      >
        <Icon name="more" size={18} />
      </button>
      <End compact={slim} />
      {menuOpen ? <Menu anchor={menuButton} onClose={closeMenu} /> : null}
    </header>
  );
}
