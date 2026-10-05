import { motion } from "motion/react";
import { useLayoutEffect, useRef, useState } from "react";
import { toBlocks } from "../core/text";
import { Icon } from "./Icon";
import { BASE, EASE_OUT } from "./motion";

/** Matches .rich__body line-height in kit.css. */
const LINE = 22;

/**
 * Shows plain text with its paragraphs and bullets, cut to a few whole lines
 * (never half a line). "Show more" opens the complete text.
 */
export function RichText({ text, lines = 6, empty, onExpand, label = "text", expandable = true }: { text: string | null | undefined; lines?: number; empty?: string; onExpand?: () => void; label?: string; expandable?: boolean }) {
  const body = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const [clampPx, setClampPx] = useState(lines * LINE);

  useLayoutEffect(() => {
    const node = body.current;
    if (!node) return;
    // Cut at the bottom of the last whole line, counting the gaps between paragraphs.
    const measure = () => {
      let left = lines;
      let cut = 0;
      for (const block of node.querySelectorAll<HTMLElement>("p, li")) {
        const take = Math.min(Math.max(1, Math.round(block.offsetHeight / LINE)), left);
        cut = block.offsetTop + take * LINE;
        left -= take;
        if (left === 0) break;
      }
      setClampPx(cut);
      setOverflows(node.scrollHeight > cut + 4);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [text, lines]);

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
