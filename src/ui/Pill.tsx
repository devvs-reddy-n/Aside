import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface PillProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  children: ReactNode;
}

/** One segment of a `PillBar` — a filter tab, a nav-active wash, a
 *  segmented choice. See DESIGN.md's "translucent segmented pill"
 *  language. Active state is a soft accent wash, never a solid fill. */
export function Pill({ active = false, className, children, ...rest }: PillProps) {
  const classes = ["u-pill", active ? "u-pill--active" : null, className]
    .filter(Boolean)
    .join(" ");
  return (
    <button type="button" className={classes} aria-pressed={active} {...rest}>
      {children}
    </button>
  );
}

export interface PillBarProps {
  children: ReactNode;
  className?: string;
}

/** The glass track a row of `Pill`s sits in (e.g. Sessions' All/Active/Ended). */
export function PillBar({ children, className }: PillBarProps) {
  const classes = ["u-pillbar", className].filter(Boolean).join(" ");
  return <div className={classes}>{children}</div>;
}
