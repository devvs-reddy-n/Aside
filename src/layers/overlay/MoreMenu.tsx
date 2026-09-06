import type { OverlayPrefs } from "../../platform/types";
import { Icon } from "../../ui/Icon";

export interface MoreMenuProps {
  /** Anchors the panel to its trigger. The caller keeps it below the top
   *  toolbar and inside the overlay, so its controls are never clipped. */
  style?: React.CSSProperties;
  prefs: OverlayPrefs;
  onChangePrefs: (next: OverlayPrefs) => void;
  onDashboard: () => void;
  onEdit: () => void;
  accountLabel: string;
}

/** ISO 639-1 codes — matches `commands::overlay::SUPPORTED_LANGUAGES`
 *  exactly, since an unrecognized code is silently replaced with `"en"` on
 *  the Rust side rather than rejected. Only actually changes transcription
 *  for Speechmatics; Parakeet has no runtime language switch. */
const LANGUAGES: Array<{ code: string; label: string }> = [
  { code: "en", label: "English" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "hi", label: "Hindi" },
  { code: "pt", label: "Portuguese" },
];

function Stepper({
  label,
  value,
  suffix,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number;
  suffix: string;
  min: number;
  max: number;
  step: number;
  onChange: (next: number) => void;
}) {
  return (
    <div className="ov-more-menu__row ov-more-menu__row--static">
      <span>{label}</span>
      <span className="ov-stepper">
        <button
          type="button"
          className="ov-stepper__btn"
          disabled={value <= min}
          onClick={() => onChange(Math.max(min, value - step))}
          aria-label={`Decrease ${label}`}
        >
          −
        </button>
        <span className="ov-stepper__value numeral">
          {value}
          {suffix}
        </span>
        <button
          type="button"
          className="ov-stepper__btn"
          disabled={value >= max}
          onClick={() => onChange(Math.min(max, value + step))}
          aria-label={`Increase ${label}`}
        >
          +
        </button>
      </span>
    </div>
  );
}

/**
 * The overflow `⋯` panel. Screen-share hiding lives in Settings, not
 * here. ⌘⇧H hides the overlay from your screen. This panel is
 * DESIGN.md's L3 "floating" level — dropdowns and popovers, one step
 * further from the glass than the toolbar's L4/L2 pair. Opacity moved out
 * to the toolbar itself (2026-08-30, always visible, matching the Stitch
 * source's own Live Overlay mockup) — Zoom and Font size stay here since
 * the reference keeps them in the overflow menu too.
 */
export function MoreMenu({
  style,
  prefs,
  onChangePrefs,
  onDashboard,
  onEdit,
  accountLabel,
}: MoreMenuProps) {
  return (
    <div className="ov-more-menu" role="menu" style={style}>
      <div className="ov-more-menu__row ov-more-menu__row--static">
        <span>{accountLabel}</span>
        <Icon name="arrowRight" className="ov-more-menu__row-icon" />
      </div>

      <button type="button" className="ov-more-menu__row" role="menuitem" onClick={onDashboard}>
        <span className="ov-more-menu__row-label">
          <Icon name="settings" />
          Dashboard
        </span>
      </button>

      <Stepper
        label="Zoom"
        value={prefs.zoom_pct}
        suffix="%"
        min={80}
        max={160}
        step={10}
        onChange={(zoom_pct) => onChangePrefs({ ...prefs, zoom_pct })}
      />

      <Stepper
        label="Font size"
        value={prefs.font_px}
        suffix="px"
        min={12}
        max={20}
        step={1}
        onChange={(font_px) => onChangePrefs({ ...prefs, font_px })}
      />

      <div className="ov-more-menu__row ov-more-menu__row--static">
        <span className="ov-more-menu__row-label">
          <Icon name="globe" />
          Language
        </span>
        <select
          value={prefs.language}
          onChange={(event) => onChangePrefs({ ...prefs, language: event.target.value })}
          className="ov-more-menu__select"
        >
          {LANGUAGES.map((lang) => (
            <option key={lang.code} value={lang.code}>
              {lang.label}
            </option>
          ))}
        </select>
      </div>

      <button type="button" className="ov-more-menu__row" role="menuitem" onClick={onEdit}>
        <span className="ov-more-menu__row-label">
          <Icon name="note" />
          Edit
        </span>
      </button>
    </div>
  );
}
