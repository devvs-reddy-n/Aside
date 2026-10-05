import { motion } from "motion/react";
import { useLayoutEffect, useRef, useState } from "react";
import { toBlocks } from "../core/text";
import { Icon } from "./Icon";
import { BASE, EASE_OUT } from "./motion";

/**
 * Shows plain text with its paragraphs and bullets, cut to a few lines with a
 * readable preview. A centered chevron opens the complete text.
 */
export function RichText({ text, lines = 6, empty, onExpand, label = "text", expandable = true }: { text: string | null | undefined; lines?: number; empty?: string; onExpand?: () => void; label?: string; expandable?: boolean }) {
  const body = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const clampPx = lines > 0 ? lines * 22 : Infinity;

  useLayoutEffect(() => {
    const node = body.current;
    if (!node) return;
    const measure = () => setOverflows(node.scrollHeight > clampPx + 4);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [text, clampPx]);

  if (!text?.trim()) return <p className="rich rich--empty">{empty}</p>;
  if (!expandable) return <p className="rich rich--preview selectable" style={{ WebkitLineClamp: lines }}>{text.replace(/\s+/g, " ").trim()}</p>;
  const blocks = toBlocks(text);
  return (
    <div className="rich">
      <motion.div
        className="rich__body selectable"
        data-clipped={overflows && !open ? "" : undefined}
        initial={false}
        animate={{ height: open || !overflows ? "auto" : clampPx }}
        transition={{ duration: BASE, ease: EASE_OUT }}
      >
        <div ref={body}>
          {blocks.map((block, index) =>
            block.kind === "para" ? (
              <p key={index}>{block.text}</p>
            ) : (
              <ul key={index}>{block.items.map((item, i) => <li key={i}>{item}</li>)}</ul>
            ),
          )}
        </div>
      </motion.div>
      {overflows ? (
        <button type="button" className="rich__more" aria-label={open ? `Collapse ${label}` : `Read full ${label}`} aria-expanded={onExpand ? undefined : open} aria-haspopup={onExpand ? "dialog" : undefined} onClick={() => onExpand ? onExpand() : setOpen(!open)}>
          <span>{open ? "Show less" : "Show more"}</span><Icon name={open ? "chevronUp" : "chevronDown"} size={14} />
        </button>
      ) : null}
    </div>
  );
}
