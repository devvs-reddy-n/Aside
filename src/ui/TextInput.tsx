import type { InputHTMLAttributes } from "react";

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  /** Monospace variant for fields where exact characters matter (API keys). */
  mono?: boolean;
}

/**
 * Single-line text input, styled with the same field shell `Select` and
 * `TextArea` already use (DESIGN.md §8 "Form field": raised background,
 * hairline border, small radius, accent focus outline). Added as a
 * primitive in Layer 1.2 — Setup's API-key fields were the first screen
 * to need a plain text input and none existed yet.
 */
export function TextInput({ mono = false, className, ...rest }: TextInputProps) {
  const classes = ["u-input", mono ? "u-input--mono" : null, className].filter(Boolean).join(" ");
  return <input className={classes} {...rest} />;
}
