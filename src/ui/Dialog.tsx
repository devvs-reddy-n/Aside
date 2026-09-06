import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { KeyboardEvent, ReactNode } from "react";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  /** Delete-with-confirm styling mode (DESIGN.md §8 "Dialog"). Destructive
   *  dialogs still require the caller to gate the confirm action itself
   *  (typed or double-click confirmation) — this only supplies the look. */
  destructive?: boolean;
  className?: string;
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Modal dialog with backdrop, focus trap and Escape-to-close.
 *  See DESIGN.md §8 "Dialog". */
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  destructive = false,
  className,
}: DialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useRef(`u-dialog-title-${Math.random().toString(36).slice(2)}`).current;

  useEffect(() => {
    if (!open) return;

    const previouslyFocused = document.activeElement as HTMLElement | null;
    const node = dialogRef.current;
    const focusable = node?.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
    (focusable?.[0] ?? node)?.focus();

    return () => {
      previouslyFocused?.focus();
    };
  }, [open]);

  if (!open) return null;

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
      return;
    }

    if (event.key !== "Tab") return;

    const node = dialogRef.current;
    if (!node) return;

    const focusable = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const classes = ["u-dialog", destructive ? "u-dialog--destructive" : null, className]
    .filter(Boolean)
    .join(" ");

  const node = (
    <div className="u-dialog-backdrop" onMouseDown={onClose}>
      <div
        ref={dialogRef}
        className={classes}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id={titleId} className="u-dialog__title">
          {title}
        </h2>
        <div className="u-dialog__body">{children}</div>
        {footer ? <div className="u-dialog__footer">{footer}</div> : null}
      </div>
    </div>
  );

  // Ported to the window root rather than rendered in place. `position:
  // fixed` is only relative to the true viewport when nothing between here
  // and it sets `backdrop-filter`/`transform`/`filter` — and this app's
  // frosted-glass chrome (`.aw-chrome`, `.aw-sheet`, and friends) does that
  // everywhere. A `Dialog` opened from inside one of those (the title bar's
  // Quit confirm is the case that surfaced this) would otherwise get its
  // "fixed, cover the window" backdrop trapped inside that ancestor's own
  // small box instead of the real window — on `.aw-chrome` (44px tall) that
  // showed as the dialog's buttons floating in a sliver at the very top of
  // the screen with no visible backdrop. Escaping to `.aw-root` (the same
  // portal host `Select` already uses, falling back to `document.body` for
  // safety) sidesteps the containing-block trap regardless of which glassy
  // ancestor a given `Dialog` happens to be opened from.
  const host =
    typeof document !== "undefined"
      ? (document.querySelector(".aw-root") ?? document.body)
      : null;
  return host ? createPortal(node, host) : node;
}
