import * as D from "@radix-ui/react-dialog";
import { useEffect, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "../bridge/runtime";
import { Icon } from "./Icon";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: string;
  /** One line under the title. */
  subtitle?: string;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg" | "wide" | "xl";
  /** Hide the close (×) button, e.g. for a required choice. */
  hideClose?: boolean;
}

/** Portaled warnings sit above sheets and bring their owning window forward. */
export function Dialog({ open, onClose, title, subtitle, children, footer, size = "md", hideClose }: DialogProps) {
  useEffect(() => {
    if (open && isTauriRuntime()) void getCurrentWindow().setFocus().catch(() => {});
  }, [open]);
  return <D.Root open={open} onOpenChange={(next) => { if (!next) onClose(); }}>
    <D.Portal>
      <D.Overlay className="dialog-backdrop" />
      <D.Content className={`dialog dialog--${size}`} aria-describedby={undefined}>
        <header className="dialog__head">
          <div className="dialog__titles"><D.Title>{title}</D.Title>{subtitle ? <D.Description>{subtitle}</D.Description> : null}</div>
          {hideClose ? null : <D.Close className="icon-btn icon-btn--sm" aria-label="Close"><Icon name="x" size={16} /></D.Close>}
        </header>
        <div className="dialog__body">{children}</div>
        {footer ? <footer className="dialog__foot">{footer}</footer> : null}
      </D.Content>
    </D.Portal>
  </D.Root>;
}
