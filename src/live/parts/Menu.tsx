import { useEffect, useRef, useState } from "react";
import * as api from "../../bridge/api";
import type { LiveLayout } from "../../core/launchPrefs";
import { APP_SHORTCUTS } from "../../core/shortcuts";
import { Icon, type IconName } from "../../kit/Icon";
import { useController, useLiveUi } from "../context";
import { MIN_OPACITY } from "../controller";

const MIN_FONT = 12;
const MAX_FONT = 20;
const MAX_OPACITY = 100;

const LAYOUTS: Array<{ value: LiveLayout; label: string; icon: IconName; hint: string }> = [
  { value: "focus", label: "Focus", icon: "layoutFocus", hint: "The question, then its answer, one at a time" },
  { value: "thread", label: "Thread", icon: "layoutThread", hint: "One conversation with answers under each question" },
  { value: "prompter", label: "Prompter", icon: "layoutPrompter", hint: "Large answer text for reading on camera" },
];

function useWindowSize(): string {
  const [size, setSize] = useState(() => `${window.innerWidth} × ${window.innerHeight}`);
  useEffect(() => {
    const read = () => setSize(`${window.innerWidth} × ${window.innerHeight}`);
    window.addEventListener("resize", read);
    return () => window.removeEventListener("resize", read);
  }, []);
  return size;
}

/** Window options. It opens inside the window and scrolls on its own, so it can never run off-screen. */
export function Menu({ onClose, anchor }: { onClose: () => void; anchor: React.RefObject<HTMLElement | null> }) {
  const controller = useController();
  const prefs = useLiveUi((state) => state.prefs);
  const layout = useLiveUi((state) => state.layout);
  const paused = useLiveUi((state) => state.paused !== null);
  const menu = useRef<HTMLDivElement>(null);
  const [hidden, setHidden] = useState<boolean | null>(null);
  const size = useWindowSize();

  useEffect(() => {
    let alive = true;
    void api.stealth.get().then((result) => alive && result.ok && setHidden(result.value));
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (menu.current?.contains(target) || anchor.current?.contains(target)) return;
      onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        anchor.current?.focus();
      }
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    menu.current?.focus();
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose, anchor]);

  return (
    <div ref={menu} className="lv-menu" role="dialog" aria-label="Window options" tabIndex={-1}>
      <p className="lv-menu__label" id="lv-layout-label">Layout</p>
      <div className="lv-layouts" role="radiogroup" aria-labelledby="lv-layout-label">
        {LAYOUTS.map((option) => (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={layout === option.value}
            title={option.hint}
            onClick={() => controller.setLayout(option.value)}
          >
            <Icon name={option.icon} size={18} strokeWidth={1.75} />
            <span>{option.label}</span>
          </button>
        ))}
      </div>

      <label className="lv-menu__row">
        <span>Background</span>
        <input
          type="range"
          min={MIN_OPACITY}
          max={MAX_OPACITY}
          value={prefs.opacity_pct}
          aria-label="Background opacity"
          aria-valuetext={`${prefs.opacity_pct}% solid`}
          onChange={(event) => controller.changePrefs({ opacity_pct: Number(event.target.value) })}
        />
        <output className="lv-menu__value tabular">{prefs.opacity_pct}%</output>
      </label>
      <div className="lv-menu__row">
        <span>Text size</span>
        <span className="lv-stepper">
          <button type="button" aria-label="Smaller text" disabled={prefs.font_px <= MIN_FONT} onClick={() => controller.changePrefs({ font_px: prefs.font_px - 1 })}>A−</button>
          <output aria-live="polite" className="tabular">{Math.round((prefs.font_px / 13) * 100)}%</output>
          <button type="button" aria-label="Larger text" disabled={prefs.font_px >= MAX_FONT} onClick={() => controller.changePrefs({ font_px: prefs.font_px + 1 })}>A+</button>
        </span>
      </div>

      <div className="lv-menu__sep" />
      <button type="button" className="lv-menu__item" onClick={() => { void controller.togglePause(); onClose(); }}>
        <Icon name={paused ? "play" : "pause"} size={15} />{paused ? "Resume listening" : "Pause listening"}
      </button>
      <button type="button" className="lv-menu__item" onClick={() => { void controller.resetWindow(); onClose(); }}>
        <Icon name="refresh" size={15} />Reset window<span className="lv-menu__meta tabular">{size}</span>
      </button>
      <button type="button" className="lv-menu__item" onClick={() => { void controller.backend.openMain(); onClose(); }}>
        <Icon name="external" size={15} />Open Aside
      </button>

      <div className="lv-menu__sep" />
      <dl className="lv-keys">
        {APP_SHORTCUTS.map(([keys, what]) => (
          <div key={keys}><dt>{what}</dt><dd><kbd>{keys}</kbd></dd></div>
        ))}
      </dl>
      <p className="lv-menu__note">
        <Icon name="shield" size={14} />
        <span>{hidden === null ? "Hiding: see Settings." : hidden ? "Hidden from screen sharing." : "Visible to screen sharing."}</span>
      </p>
    </div>
  );
}
