import type { ReactNode } from "react";

export interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  htmlFor?: string;
  className?: string;
  children: ReactNode;
}

/** Labelled form field wrapper. See DESIGN.md §8 "Form field". Helper text
 *  and error text are mutually exclusive; error replaces the hint, never both. */
export function Field({ label, hint, error, htmlFor, className, children }: FieldProps) {
  const classes = ["u-field", className].filter(Boolean).join(" ");

  return (
    <div className={classes}>
      <label className="u-field__label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {error ? (
        <p className="u-field__error">{error}</p>
      ) : hint ? (
        <p className="u-field__hint">{hint}</p>
      ) : null}
    </div>
  );
}
