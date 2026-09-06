import { useEffect, useRef } from "react";
import { Icon } from "../ui/Icon";
import { SettingsLayer } from "../layers/settings/SettingsLayer";

export interface SettingsSheetProps {
  open: boolean;
  onClose: () => void;
}

/**
 * Settings as an L4 sheet floating over the window (DESIGN.md §2 L4 + §8
 * "Modal / settings sheet": `scale(.98) → 1` + opacity, 220ms, centre
 * origin — a sheet has no single trigger button to anchor from, since
 * it's opened from the chrome's gear icon on one edge but covers the
 * whole window). This is the fix for "keep settings toggle where I can
 * enter my APIs" — the gear in `WindowChrome` is the one, obvious way in,
 * and the header's back arrow plus Escape/backdrop-click are the "clear
 * way back" the brief asks for. `SettingsLayer` and every card under it
 * (`ProviderModelCard`, `SpeechEngineCard`, ...) are unchanged logic —
 * this only supplies the floating shell around them.
 */
export function SettingsSheet({ open, onClose }: SettingsSheetProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Nested full-window pages (answer-rules warning/editor) and dialogs
      // own Escape. Don't close the whole sheet out from under them.
      if (document.querySelector(".settings-full-window, .u-dialog")) return;
      onClose();
    }
    document.addEventListener("keydown", onKeyDown);
    panelRef.current?.focus();
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="aw-sheet-backdrop" onMouseDown={onClose}>
      <div
        className="aw-sheet"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        tabIndex={-1}
        ref={panelRef}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="aw-sheet__head">
          <button type="button" className="aw-sheet__back" onClick={onClose} aria-label="Back to Create">
            <Icon name="chevronLeft" />
            <span>Back</span>
          </button>
          <button type="button" className="aw-sheet__close" onClick={onClose} aria-label="Close settings">
            <Icon name="close" />
          </button>
        </div>
        <div className="aw-sheet__body">
          <SettingsLayer />
        </div>
      </div>
    </div>
  );
}
