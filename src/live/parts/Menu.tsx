import { useEffect, useRef } from "react";
import { Icon } from "../../kit/Icon";
import { useController, useLiveUi } from "../context";
import { MIN_OPACITY } from "../controller";

/** Text sizes for the question and answer, in the saved font_px units (the backend allows 12 to 20). */
const SIZES: Array<[number, string]> = [
  [13, "Small"],
  [15, "Medium"],
  [17, "Large"],
  [20, "Extra large"],
];

/** The saved size closest to one of the four choices, so an older in-between value still shows a selection. */
function nearestSize(px: number): number {
  return SIZES.reduce((best, [size]) => (Math.abs(size - px) < Math.abs(best - px) ? size : best), SIZES[0][0]);
}

/** Window options. It opens inside the window and scrolls on its own, so it can never run off-screen. */
export function Menu({ onClose, anchor }: { onClose: () => void; anchor: React.RefObject<HTMLElement | null> }) {
  const controller = useController();
  const prefs = useLiveUi((state) => state.prefs);
  const paused = useLiveUi((state) => state.paused !== null);
  const menu = useRef<HTMLDivElement>(null);
  const size = nearestSize(prefs.font_px);

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
      <label className="lv-menu__row">
        <span>Background</span>
        <input
          type="range"
          min={MIN_OPACITY}
          max={100}
          value={prefs.opacity_pct}
          aria-label="Background opacity"
          aria-valuetext={`${prefs.opacity_pct}% solid`}
          onChange={(event) => controller.changePrefs({ opacity_pct: Number(event.target.value) })}
        />
        <output className="lv-menu__value tabular">{prefs.opacity_pct}%</output>
      </label>
      <div className="lv-menu__row">
        <span id="lv-size-label">Text size</span>
        <span className="lv-sizes" role="radiogroup" aria-labelledby="lv-size-label">
          {SIZES.map(([px, name]) => (
            <button key={px} type="button" role="radio" aria-checked={size === px} aria-label={name} title={name} style={{ fontSize: `${px - 1}px` }} onClick={() => controller.changePrefs({ font_px: px })}>A</button>
          ))}
        </span>
      </div>

      <div className="lv-menu__sep" />
      <button type="button" className="lv-menu__item" onClick={() => { void controller.togglePause(); onClose(); }}>
        <Icon name={paused ? "play" : "pause"} size={15} />{paused ? "Resume listening" : "Pause listening"}
      </button>
      <button type="button" className="lv-menu__item" onClick={() => { void controller.resetWindow(); onClose(); }}>
        <Icon name="refresh" size={15} />Reset window
      </button>
      <button type="button" className="lv-menu__item" onClick={() => { void controller.backend.openMain(); onClose(); }}>
        <Icon name="external" size={15} />Open Aside
      </button>
    </div>
  );
}
