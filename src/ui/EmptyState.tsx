import type { ReactNode } from "react";

export interface EmptyStateProps {
  heading: ReactNode;
  description: ReactNode;
  action?: ReactNode;
  className?: string;
}

/** Composed empty state: what's missing, how to populate it, optional
 *  action. Type and spacing only, no illustration. DESIGN.md §8, §10. */
export function EmptyState({ heading, description, action, className }: EmptyStateProps) {
  const classes = ["u-empty", className].filter(Boolean).join(" ");

  return (
    <div className={classes}>
      <h3 className="u-empty__heading">{heading}</h3>
      <p className="u-empty__description">{description}</p>
      {action ? <div className="u-empty__action">{action}</div> : null}
    </div>
  );
}
