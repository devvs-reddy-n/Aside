import type { ReactNode } from "react";

export type StatusTone = "neutral" | "ok" | "warn" | "bad";

export interface StatusOrbProps {
  tone?: StatusTone;
  /** Slow breathing glow — reserved for the one "this is live right now"
   *  state (DESIGN.md: never more than one continuous animation). */
  live?: boolean;
  children?: ReactNode;
  className?: string;
}

/** A small dot, optionally labelled — the sidebar's mic/system/model/
 *  connection readout, and the overlay's Listening indicator. Replaces
 *  large diagnostic panels with the spec's "small status dots" language. */
export function StatusOrb({ tone = "neutral", live = false, children, className }: StatusOrbProps) {
  const orbClasses = [
    "u-status-orb",
    tone !== "neutral" ? `u-status-orb--${tone}` : null,
    live ? "u-status-orb--live" : null,
  ]
    .filter(Boolean)
    .join(" ");
  const wrapClasses = ["u-status", className].filter(Boolean).join(" ");

  return (
    <span className={wrapClasses}>
      <span className={orbClasses} aria-hidden="true" />
      {children}
    </span>
  );
}
