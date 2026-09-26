import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger" | "quiet";
  size?: "sm" | "md" | "lg";
  icon?: IconName;
  iconRight?: IconName;
  busy?: boolean;
  children?: ReactNode;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  iconRight,
  busy = false,
  children,
  className,
  disabled,
  type = "button",
  ...rest
}: ButtonProps) {
  const classes = ["btn", `btn--${variant}`, `btn--${size}`, className].filter(Boolean).join(" ");
  return (
    <button {...rest} type={type} className={classes} disabled={disabled || busy} aria-busy={busy || undefined}>
      {busy ? <span className="spinner" aria-hidden="true" /> : icon ? <Icon name={icon} size={size === "sm" ? 15 : 17} /> : null}
      {children ? <span className="btn__label">{children}</span> : null}
      {iconRight && !busy ? <Icon name={iconRight} size={size === "sm" ? 15 : 17} /> : null}
    </button>
  );
}

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
  size?: "sm" | "md";
  tone?: "default" | "blue" | "danger";
  active?: boolean;
}

export function IconButton({ icon, label, size = "md", tone = "default", active, className, type = "button", ...rest }: IconButtonProps) {
  const classes = ["icon-btn", `icon-btn--${size}`, tone !== "default" && `icon-btn--${tone}`, className].filter(Boolean).join(" ");
  return (
    <button {...rest} type={type} className={classes} aria-label={label} title={rest.title ?? label} aria-pressed={active}>
      <Icon name={icon} size={size === "sm" ? 16 : 18} />
    </button>
  );
}
