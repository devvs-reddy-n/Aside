import { APP_SHORTCUTS } from "../core/shortcuts";
import { useEffect, useRef, useState } from "react";
import * as api from "../bridge/api";
import { Icon } from "../kit/Icon";
import { useController, useLiveUi } from "./context";
import { MIN_OPACITY } from "./controller";

const MIN_FONT = 12;
const MAX_FONT = 20;
const MAX_OPACITY = 100;

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
      <button type="button" className="lv-menu__item" onClick={() => { void controller.togglePause(); onClose(); }}>
        <Icon name={paused ? "play" : "stop"} size={15} />{paused ? "Resume listening" : "Pause listening"}
      </button>

      <div className="lv-menu__row">
        <span>Text size</span>
        <span className="lv-stepper">
          <button type="button" aria-label="Smaller text" disabled={prefs.font_px <= MIN_FONT} onClick={() => controller.changePrefs({ font_px: prefs.font_px - 1 })}>A−</button>
          <output aria-live="polite">{Math.round((prefs.font_px / 13) * 100)}%</output>
          <button type="button" aria-label="Larger text" disabled={prefs.font_px >= MAX_FONT} onClick={() => controller.changePrefs({ font_px: prefs.font_px + 1 })}>A+</button>
        </span>
      </div>
      <label className="lv-menu__row">
        <span>Opacity</span>
        <input
          type="range"
          min={MIN_OPACITY}
          max={MAX_OPACITY}
          value={prefs.opacity_pct}
          aria-label="Window opacity"
          onChange={(event) => controller.changePrefs({ opacity_pct: Number(event.target.value) })}
        />
      </label>
      

      <div className="lv-menu__sep" />
      <div className="lv-menu__row">
        <span>Window</span>
        <span className="lv-menu__size">{size}</span>
      </div>
      <button type="button" className="lv-menu__item" onClick={() => { void controller.resetWindow(); onClose(); }}>
        <Icon name="refresh" size={15} />Reset window
      </button>
      

      <div className="lv-menu__sep" />
      <p className="lv-menu__note lv-menu__note--icon">
        <Icon name="shield" size={15} />
        <span>
          {hidden === null
            ? "Hiding: see Settings."
            : hidden
              ? "Hidden from screen sharing."
              : "Visible to screen sharing."}
        </span>
      </p>

      <div className="lv-menu__sep" />
      <dl className="lv-keys">
        {APP_SHORTCUTS.map(([keys, what]) => (
          <div key={keys}><dt>{what}</dt><dd><kbd>{keys}</kbd></dd></div>
        ))}
      </dl>

      <div className="lv-menu__sep" />
      <button type="button" className="lv-menu__item" onClick={() => { void controller.backend.openMain(); onClose(); }}>
        <Icon name="external" size={15} />Open Aside
      </button>
    </div>
  );
}
