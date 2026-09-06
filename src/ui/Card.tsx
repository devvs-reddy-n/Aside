import type { HTMLAttributes, ReactNode } from "react";

export interface CardProps extends Omit<HTMLAttributes<HTMLDivElement>, "title"> {
  eyebrow?: ReactNode;
  title?: ReactNode;
  footer?: ReactNode;
  /** Large radius is reserved for the Live Answer card (DESIGN.md §5). */
  size?: "md" | "xl";
  /** Most surfaces are correctly flat; only add a border when the fill alone
   *  does not separate the card from the canvas (DESIGN.md §8). */
  bordered?: boolean;
  children: ReactNode;
}

/** Panel surface with an optional header and footer. See DESIGN.md §8 "Card / panel". */
export function Card({
  eyebrow,
  title,
  footer,
  size = "md",
  bordered = false,
  className,
  children,
  ...rest
}: CardProps) {
  const classes = [
    "u-card",
    size === "xl" ? "u-card--xl" : null,
    bordered ? "u-card--bordered" : null,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  const hasHeader = Boolean(eyebrow || title);

  return (
    <div className={classes} {...rest}>
      {hasHeader ? (
        <header className="u-card__head">
          {eyebrow ? <p className="u-card__eyebrow">{eyebrow}</p> : null}
          {title ? <h2 className="u-card__title">{title}</h2> : null}
        </header>
      ) : null}
      <div className="u-card__body">{children}</div>
      {footer ? <footer className="u-card__foot">{footer}</footer> : null}
    </div>
  );
}
