import { Icon } from "../../ui/Icon";

export interface SubBarProps {
  canPrev: boolean;
  canNext: boolean;
  onPrev: () => void;
  onNext: () => void;
  onClear: () => void;
  onCopy: () => void;
  generating: boolean;
}

/** History nav + Auto Answer/Clear/Copy — only rendered while the body has
 *  content. Same `.ov-pill` L2 controls as the toolbar above it. */
export function SubBar({
  canPrev,
  canNext,
  onPrev,
  onNext,
  onClear,
  onCopy,
  generating,
}: SubBarProps) {
  return (
    <div className="ov-subbar">
      <button type="button" className="ov-pill ov-pill--icon" title="Previous answer (⌘[)" disabled={!canPrev} onClick={onPrev}>
        <Icon name="chevronLeft" />
      </button>
      <button type="button" className="ov-pill ov-pill--icon" title="Next answer (⌘])" disabled={!canNext} onClick={onNext}>
        <Icon name="chevronRight" />
      </button>

      {generating ? <span className="ov-generating-chip">Generating…</span> : null}

      <div className="ov-subbar__right">
        <button type="button" className="ov-pill" title="Clear this answer" onClick={onClear}>
          <Icon name="trash" />
          Clear
        </button>
        <button type="button" className="ov-pill" title="Copy" onClick={onCopy}>
          <Icon name="copy" />
          Copy
        </button>
      </div>
    </div>
  );
}
