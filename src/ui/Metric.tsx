import type { HTMLAttributes, ReactNode } from "react";

export interface MetricProps extends HTMLAttributes<HTMLDivElement> {
  label: ReactNode;
  value: ReactNode;
  className?: string;
}

/** Label plus a monospace tabular-nums value, for Cache & Timing readouts.
 *  See DESIGN.md §8 "Table / metric readout". */
export function Metric({ label, value, className, ...rest }: MetricProps) {
  const classes = ["u-metric", className].filter(Boolean).join(" ");

  return (
    <div className={classes} {...rest}>
      <p className="u-metric__label">{label}</p>
      <p className="u-metric__value">{value}</p>
    </div>
  );
}
