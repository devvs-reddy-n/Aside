import { useLayoutEffect, useRef } from "react";

/** Keep wrapped text visible after editing, window resizing, or sidebar resizing. */
export function useAutoHeight(value: unknown) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    const fit = () => {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight + node.offsetHeight - node.clientHeight}px`;
    };
    fit();
    let width = node.getBoundingClientRect().width;
    const observer = new ResizeObserver(() => {
      const next = node.getBoundingClientRect().width;
      if (next === width) return;
      width = next;
      fit();
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [value]);
  return ref;
}
