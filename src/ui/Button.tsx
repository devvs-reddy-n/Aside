import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "destructive";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  /** Acheron's 45° corner-cut accent — reserve for marquee CTAs (Start Call,
   * Initialize/Save Copilot), not every button. */
  chamfer?: boolean;
  children: ReactNode;
}

/** Pill action button. See DESIGN.md §8 "Button". */
export function Button({
  variant = "secondary",
  chamfer = false,
  className,
  children,
  ...rest
}: ButtonProps) {
  const classes = ["u-btn", `u-btn--${variant}`, chamfer && "u-btn--chamfer", className]
    .filter(Boolean)
    .join(" ");

  return (
    <button className={classes} {...rest}>
      {children}
    </button>
  );
}
