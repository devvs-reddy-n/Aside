import type { TextareaHTMLAttributes } from "react";

export interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  /** Monospace variant for fields where exact characters matter (API keys,
   *  response-style instructions, the Advanced rules editor). DESIGN.md §8. */
  mono?: boolean;
  /** Shows a live character count under the field. Pairs with `maxLength`. */
  showCount?: boolean;
}

/** Styled textarea. See DESIGN.md §8 "Textarea". */
export function TextArea({
  mono = false,
  showCount = false,
  className,
  value,
  maxLength,
  ...rest
}: TextAreaProps) {
  const classes = ["u-textarea", mono ? "u-textarea--mono" : null, className]
    .filter(Boolean)
    .join(" ");

  const length = typeof value === "string" ? value.length : 0;

  return (
    <div className="u-textarea-wrap">
      <textarea className={classes} value={value} maxLength={maxLength} {...rest} />
      {showCount ? (
        <p className="u-textarea-count">
          {length}
          {typeof maxLength === "number" ? ` / ${maxLength}` : ""}
        </p>
      ) : null}
    </div>
  );
}
