import * as D from "@radix-ui/react-dialog";
import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { IconButton } from "./Button";
import { EASE_OUT, FAST, SLOW } from "./motion";

/**
 * A panel that slides in from the right over a softly dimmed window. Used for
 * anything that needs room: long-text editors, the start-session checks.
 */
export function Sheet({ open, onClose, title, meta, children, footer, width = 600 }: {
  open: boolean; onClose: () => void; title: string; meta?: ReactNode; children: ReactNode; footer?: ReactNode; width?: number;
}) {
  return (
    <D.Root open={open} onOpenChange={(next) => { if (!next) onClose(); }}>
      <AnimatePresence>
        {open ? (
          <D.Portal forceMount>
            <D.Overlay asChild forceMount>
              <motion.div className="sheet-backdrop" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }} />
            </D.Overlay>
            <D.Content asChild forceMount aria-describedby={undefined}>
              <motion.div
                className="sheet"
                style={{ width: `min(${width}px, calc(100vw - 48px))` }}
                initial={{ x: 24, opacity: 0 }}
                animate={{ x: 0, opacity: 1 }}
                exit={{ x: 24, opacity: 0 }}
                transition={{ duration: SLOW, ease: EASE_OUT }}
              >
                <header className="sheet__head">
                  <D.Title className="sheet__title">{title}</D.Title>
                  {meta ? <span className="sheet__meta">{meta}</span> : null}
                  <span className="grow" />
                  <D.Close asChild>
                    <IconButton icon="x" label="Close" size="sm" />
                  </D.Close>
                </header>
                <div className="sheet__body">{children}</div>
                {footer ? <footer className="sheet__foot">{footer}</footer> : null}
              </motion.div>
            </D.Content>
          </D.Portal>
        ) : null}
      </AnimatePresence>
    </D.Root>
  );
}
