import * as D from "@radix-ui/react-dialog";
import { useEffect, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "../bridge/runtime";
import { Icon } from "./Icon";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: string;
  /** Under the title: a line of text or a row of chips. */
  subtitle?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg" | "wide" | "xl";
  /** Hide the close (×) button, e.g. for a required choice. */
  hideClose?: boolean;
  /** A small picture before the title (for example a monogram that glides in from a list). */
  icon?: ReactNode;
}

/** Portaled warnings sit above sheets and bring their owning window forward. */
export function Dialog({ open, onClose, title, subtitle, children, footer, size = "md", hideClose, icon }: DialogProps) {
  useEffect(() => {
    if (open && isTauriRuntime()) void getCurrentWindow().setFocus().catch(() => {});
  }, [open]);
  return <D.Root open={open} onOpenChange={(next) => { if (!next) onClose(); }}>
    <D.Portal>
      <D.Overlay className="dialog-backdrop" />
      <D.Content className={`dialog dialog--${size}`} aria-describedby={undefined}>
        <header className="dialog__head">
          {icon ? <span className="dialog__icon">{icon}</span> : null}
          <div className="dialog__titles"><D.Title>{title}</D.Title>{subtitle ? <D.Description asChild><div className="dialog__sub">{subtitle}</div></D.Description> : null}</div>
          {hideClose ? null : <D.Close className="icon-btn icon-btn--sm" aria-label="Close"><Icon name="x" size={16} /></D.Close>}
        </header>
        <div className="dialog__body">{children}</div>
        {footer ? <footer className="dialog__foot">{footer}</footer> : null}
      </D.Content>
    </D.Portal>
  </D.Root>;
}
