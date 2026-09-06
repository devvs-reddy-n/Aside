export interface LevelMeterProps {
  /** 0 to 100. Values outside that range are clamped. */
  value: number;
  /** Small caption under the bar, e.g. "quiet" / "loud enough". */
  label?: string;
  className?: string;
}

/**
 * Horizontal loudness bar for a live microphone / system-audio reading.
 * Added to the primitive set in Layer 1.2 — Setup's live meters were the
 * first screen to need one, and no `u-*` component covered it yet.
 * Follows the same shape as every other primitive here: a `u-` prefixed
 * class, tokens only, no inline styling except the one live-updating
 * `transform` the meter itself needs.
 */
export function LevelMeter({ value, label, className }: LevelMeterProps) {
  const clamped = Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0;
  const classes = ["u-meter", className].filter(Boolean).join(" ");

  return (
    <div className={classes}>
      <div className="u-meter__track" role="meter" aria-valuenow={Math.round(clamped)} aria-valuemin={0} aria-valuemax={100}>
        <span className="u-meter__fill" style={{ transform: `scaleX(${clamped / 100})` }} />
      </div>
      {label ? <p className="u-meter__label">{label}</p> : null}
    </div>
  );
}
