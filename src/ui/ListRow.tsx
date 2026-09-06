import type { HTMLAttributes, ReactNode } from "react";

export interface ListRowProps extends Omit<HTMLAttributes<HTMLDivElement>, "title"> {
  leading?: ReactNode;
  title: ReactNode;
  meta?: ReactNode;
  trailing?: ReactNode;
  selected?: boolean;
  className?: string;
}

/** Flat row for lists (Past Sessions, Memory facts). See DESIGN.md §8
 *  "List row" — bottom-edge hairline only, hover lifts the fill, no shadow. */
export function ListRow({
  leading,
  title,
  meta,
  trailing,
  selected = false,
  className,
  ...rest
}: ListRowProps) {
  const classes = ["u-row", selected ? "u-row--selected" : null, className]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} {...rest}>
      {leading ? <div className="u-row__leading">{leading}</div> : null}
      <div className="u-row__main">
        <div className="u-row__title">{title}</div>
        {meta ? <div className="u-row__meta">{meta}</div> : null}
      </div>
      {trailing ? <div className="u-row__trailing">{trailing}</div> : null}
    </div>
  );
}
