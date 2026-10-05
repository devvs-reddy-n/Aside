import { useCallback, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { monogram, tintHue } from "../../core/text";
import { Icon } from "../../kit/Icon";
import { useLive } from "../context";
import { noMaximize, useElapsed } from "../hooks";
import { End } from "./End";
import { Menu } from "./Menu";
import { usePhase } from "./status";

/**
 * The top edge, in three parts: who the call is for on the left, a quiet
 * status (dot, word, clock) in the middle, and the window menu and End on the
 * right. Empty space drags the window. `slim` drops the name and the status
 * word so the Prompter and tiny windows keep their room; `start` and `end`
 * add controls to either side.
 */
export function TopBar({ slim = false, start, end }: { slim?: boolean; start?: ReactNode; end?: ReactNode }) {
  const elapsed = useElapsed();
  const { phase, tone, text } = usePhase();
  const company = useLive((state) => state.session?.company?.trim() || "Live session");
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);
  const pulse = phase === "listening" || phase === "answering";

  return (
    <header className="lv-top" data-slim={slim || undefined} data-tauri-drag-region {...noMaximize}>
      <div className="lv-top__start" data-tauri-drag-region>
        {slim ? null : (
          <span className="lv-top__who" data-tauri-drag-region>
            <span className="lv-top__mono" style={{ "--h": tintHue(company) } as CSSProperties} aria-hidden="true">{monogram(company)}</span>
            <b data-tauri-drag-region title={company}>{company}</b>
          </span>
        )}
        {start}
      </div>
      <span className="lv-status" data-tone={tone} role="status" aria-label={`${text}. Time on this call ${elapsed}`} data-tauri-drag-region>
        <i className="lv-status__dot" data-pulse={pulse || undefined} aria-hidden="true" />
        {slim ? null : <span className="lv-status__text" key={text}>{text}</span>}
        <time className="tabular">{elapsed}</time>
      </span>
      <div className="lv-top__end" data-tauri-drag-region>
        {end}
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
      </div>
      {menuOpen ? <Menu anchor={menuButton} onClose={closeMenu} /> : null}
    </header>
  );
}
