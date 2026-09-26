import { useLayoutEffect, useRef, type TextareaHTMLAttributes } from "react";

/** A plain, roomy text editor that grows with its text instead of showing a scrollbar. */
export function TextEditor(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }, [props.value]);
  return <textarea ref={ref} spellCheck {...props} className={["editor", props.className].filter(Boolean).join(" ")} />;
}
