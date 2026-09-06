import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "../../ui/Icon";

export interface SettingsFullWindowProps {
  open: boolean;
  title: string;
  onBack: () => void;
  onClose: () => void;
  footer?: ReactNode;
  children: ReactNode;
}

/**
 * A second full-window sheet stacked on top of Settings. Same chrome as
 * Settings itself — not a small centered dialog.
 */
export function SettingsFullWindow({
  open,
  title,
  onBack,
  onClose,
  footer,
  children,
}: SettingsFullWindowProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (document.querySelector(".u-dialog")) return;
      event.stopPropagation();
      onBack();
    }
    document.addEventListener("keydown", onKeyDown, true);
    panelRef.current?.focus();
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [open, onBack]);

  if (!open) return null;

  const host = document.querySelector(".aw-root");
  if (!host) return null;

  return createPortal(
    <div className="aw-sheet-backdrop settings-full-window" onMouseDown={onBack}>
      <div
        className="aw-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        ref={panelRef}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="aw-sheet__head">
          <button type="button" className="aw-sheet__back" onClick={onBack} aria-label="Back">
            <Icon name="chevronLeft" />
            <span>Back</span>
          </button>
          <h1 className="settings-full-window__title">{title}</h1>
          <button type="button" className="aw-sheet__close" onClick={onClose} aria-label="Close">
            <Icon name="close" />
          </button>
        </div>
        <div className="aw-sheet__body settings-full-window__body">{children}</div>
        {footer ? <div className="settings-full-window__foot">{footer}</div> : null}
      </div>
    </div>,
    host,
  );
}
