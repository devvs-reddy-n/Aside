import * as RSwitch from "@radix-ui/react-switch";
import * as TG from "@radix-ui/react-toggle-group";
import { motion } from "motion/react";
import { SPRING } from "./motion";
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

/** A labelled on/off row: label and helper on the left, switch on the right. */
export function Switch({ checked, onChange, label, hint, disabled }: SwitchProps) {
  const id = useId();
  return (
    <div className="switch-row">
      <label htmlFor={id} className="switch-row__text">
        <span className="switch-row__label">{label}</span>
        {hint ? <span className="switch-row__hint">{hint}</span> : null}
      </label>
      <Toggle id={id} checked={checked} onChange={onChange} label={label} disabled={disabled} />
    </div>
  );
}

/** A bare on/off switch for rows that already show their own label. The knob moves on a spring. */
export function Toggle({ checked, onChange, label, disabled, id }: { checked: boolean; onChange: (next: boolean) => void; label: string; disabled?: boolean; id?: string }) {
  return (
    <RSwitch.Root id={id} className="switch" checked={checked} onCheckedChange={onChange} aria-label={label} disabled={disabled}>
      <RSwitch.Thumb asChild>
        <motion.span className="switch__knob" layout transition={SPRING} />
      </RSwitch.Thumb>
    </RSwitch.Root>
  );
}

export interface SegmentedProps<T extends string> {
  value: T;
  onChange: (next: T) => void;
  options: Array<{ value: T; label: ReactNode; title?: string }>;
  label: string;
  size?: "sm" | "md";
}

/** Choose one of a few. The highlight slides to the chosen option. */
export function Segmented<T extends string>({ value, onChange, options, label, size = "md" }: SegmentedProps<T>) {
  const id = useId();
  return (
    <TG.Root type="single" className={`seg seg--${size}`} value={value} aria-label={label} onValueChange={(next) => { if (next) onChange(next as T); }}>
      {options.map((option) => (
        <TG.Item key={option.value} value={option.value} className="seg__item" title={option.title} aria-label={typeof option.label === "string" ? undefined : option.title}>
          {value === option.value ? <motion.span layoutId={`seg-${id}`} className="seg__thumb" transition={SPRING} /> : null}
          <span className="seg__label">{option.label}</span>
        </TG.Item>
      ))}
    </TG.Root>
  );
}
