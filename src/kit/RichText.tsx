import { motion } from "motion/react";
import { useLayoutEffect, useRef, useState } from "react";
import { toBlocks } from "../core/text";
import { BASE, EASE_OUT } from "./motion";

/**
 * Shows plain text with its paragraphs and bullets, cut to a few lines with a
 * soft fade. "Show all" opens the rest in place.
 */
export function RichText({ text, lines = 6, empty }: { text: string | null | undefined; lines?: number; empty?: string }) {
  const body = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const clampPx = lines * 22;

  useLayoutEffect(() => {
    const node = body.current;
    if (node) setOverflows(node.scrollHeight > clampPx + 4);
  }, [text, clampPx]);

  if (!text?.trim()) return <p className="rich rich--empty">{empty}</p>;
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
        <button type="button" className="rich__more" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? "Show less" : "Show all"}
        </button>
      ) : null}
    </div>
  );
}
