import type { HTMLAttributes, ReactNode } from "react";

export interface TableProps extends HTMLAttributes<HTMLDivElement> {
  ariaLabel: string;
  children: ReactNode;
}

/**
 * Shared list-table shell (Call History, Copilots, Documents/Resume). One
 * CSS Grid spans the header row and every data row — see `TableRow`'s
 * `display: contents`. That is what makes column alignment automatic: the
 * browser measures every column from every row's cells *together*, so a
 * row whose content differs in width from the others (icon buttons vs. a
 * text button vs. an empty header cell; a long file name vs. a short one)
 * still lands in exactly the same column as the header and every other
 * row — no per-column pixel guess anywhere.
 *
 * The bug this replaces: each row used to draw its *own independent* grid
 * (`.xx-table` as a flex column of separately-gridded `.xx-table__row`
 * boxes). `fr`/`auto` tracks size from whatever is in that one row, so two
 * rows with different content pulled different pixel widths for the same
 * column even though the CSS rule text was identical between them.
 *
 * Each page still owns its own column widths — pass a `className` (e.g.
 * `ss-table`) and set `grid-template-columns` on that class in the page's
 * own stylesheet, same as before. Only the "one independent grid per row"
 * structure is gone.
 */
export function Table({ ariaLabel, className, children, ...rest }: TableProps) {
  const classes = ["u-table", className].filter(Boolean).join(" ");
  return (
    <div className={classes} role="table" aria-label={ariaLabel} {...rest}>
      {children}
    </div>
  );
}

export interface TableRowProps extends HTMLAttributes<HTMLDivElement> {
  /** Marks the header row — adds `u-table__row--head` alongside whatever
   *  page-specific head class (e.g. `ss-row--head`) the caller passes. */
  head?: boolean;
  children: ReactNode;
}

/**
 * One logical row. `display: contents` (`.u-table__row` in `ui.css`) makes
 * this div invisible to layout — its cell children become direct grid
 * items of the parent `Table`, which is what lets every row share one set
 * of column tracks instead of each computing its own.
 *
 * A `display: contents` element can't paint a background, border, or
 * padding of its own (the box simply doesn't exist), so per-row spacing
 * and dividers that used to live on the row now live on `.xx-table__row >
 * *` (each cell) in the page's own stylesheet.
 */
export function TableRow({ head = false, className, children, ...rest }: TableRowProps) {
  const classes = ["u-table__row", head ? "u-table__row--head" : null, className]
    .filter(Boolean)
    .join(" ");
  return (
    <div className={classes} role="row" {...rest}>
      {children}
    </div>
  );
}
