import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { Icon, type IconName } from "./Icon";
import { Tip } from "./Tooltip";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger" | "quiet";
  size?: "sm" | "md" | "lg";
  icon?: IconName;
  iconRight?: IconName;
  busy?: boolean;
  children?: ReactNode;
  ref?: Ref<HTMLButtonElement>;
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
  const classes = ["btn", `btn--${variant}`, `btn--${size}`, !children && "btn--icon", className].filter(Boolean).join(" ");
  const iconSize = size === "sm" ? 15 : 16;
  return (
    <button {...rest} type={type} className={classes} disabled={disabled || busy} aria-busy={busy || undefined}>
      {busy ? <span className="spinner" aria-hidden="true" /> : icon ? <Icon name={icon} size={iconSize} /> : null}
      {children ? <span className="btn__label">{children}</span> : null}
      {iconRight && !busy ? <Icon name={iconRight} size={iconSize} /> : null}
    </button>
  );
}

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  /** Spoken name and tooltip. Every icon-only button has one. */
  label: string;
  /** Extra tooltip text, e.g. a shortcut. */
  hint?: string;
  size?: "sm" | "md";
  tone?: "default" | "blue" | "danger";
  active?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

export function IconButton({ icon, label, hint, size = "md", tone = "default", active, className, type = "button", ...rest }: IconButtonProps) {
  const classes = ["icon-btn", `icon-btn--${size}`, tone !== "default" && `icon-btn--${tone}`, className].filter(Boolean).join(" ");
  return (
    <Tip label={hint ? `${label}  ${hint}` : label}>
      <button {...rest} type={type} className={classes} aria-label={label} aria-pressed={active}>
        <Icon name={icon} size={size === "sm" ? 15 : 17} />
      </button>
    </Tip>
  );
}
