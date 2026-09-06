import type { HTMLAttributes, ReactNode } from "react";

export type BadgeTone = "neutral" | "accent" | "ok" | "warn" | "bad";

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: BadgeTone;
  children: ReactNode;
}

/** Small pill label. See DESIGN.md §8 "Badge / pill". No coloured status
 *  dot ever accompanies this (§10 avoid list) — the tone colour on the text
 *  itself is the only signal. */
export function Badge({ tone = "neutral", className, children, ...rest }: BadgeProps) {
  const classes = ["u-badge", `u-badge--${tone}`, className].filter(Boolean).join(" ");

  return (
    <span className={classes} {...rest}>
      {children}
    </span>
  );
}
