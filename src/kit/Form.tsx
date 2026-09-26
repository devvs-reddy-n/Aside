import { useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";

interface FieldProps {
  label: string;
  hint?: ReactNode;
  error?: string | null;
  /** Small text shown at the right of the label row, e.g. "Optional". */
  aside?: ReactNode;
  children: (id: string) => ReactNode;
  className?: string;
}

export function Field({ label, hint, error, aside, children, className }: FieldProps) {
  const id = useId();
  return (
    <div className={["field", className].filter(Boolean).join(" ")}>
      <div className="field__row">
        <label htmlFor={id} className="field__label">{label}</label>
        {aside ? <span className="field__aside">{aside}</span> : null}
      </div>
      {children(id)}
      {error ? <p className="field__error" role="alert">{error}</p> : hint ? <p className="field__hint">{hint}</p> : null}
    </div>
  );
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={["input", props.className].filter(Boolean).join(" ")} />;
}

export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} className={["input input--area", props.className].filter(Boolean).join(" ")} />;
}

export function Select({ children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <span className="select">
      <select {...props} className={["input", props.className].filter(Boolean).join(" ")}>{children}</select>
      <svg className="select__chev" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
    </span>
  );
}

export interface SwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  hint?: ReactNode;
  disabled?: boolean;
}

/** A labelled on/off switch. The whole row is the click target. */
export function Switch({ checked, onChange, label, hint, disabled }: SwitchProps) {
  return (
    <button type="button" role="switch" aria-checked={checked} disabled={disabled} className="switch-row" onClick={() => onChange(!checked)}>
      <span className="switch-row__text">
        <span className="switch-row__label">{label}</span>
        {hint ? <span className="switch-row__hint">{hint}</span> : null}
      </span>
      <span className={`switch${checked ? " switch--on" : ""}`} aria-hidden="true"><span className="switch__knob" /></span>
    </button>
  );
}

/** A bare on/off switch for rows that already show their own label. */
export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (next: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`switch${checked ? " switch--on" : ""}`} onClick={() => onChange(!checked)}>
      <span className="switch__knob" />
    </button>
  );
}

export interface SegmentedProps<T extends string> {
  value: T;
  onChange: (next: T) => void;
  options: Array<{ value: T; label: string }>;
  label: string;
}

export function Segmented<T extends string>({ value, onChange, options, label }: SegmentedProps<T>) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button key={option.value} type="button" role="radio" aria-checked={value === option.value} className="segmented__item" onClick={() => onChange(option.value)}>
          {option.label}
        </button>
      ))}
    </div>
  );
}
