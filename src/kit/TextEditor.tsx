import type { TextareaHTMLAttributes } from "react";
import { useAutoHeight } from "./useAutoHeight";

/** A plain, roomy text editor that grows with its text instead of showing a scrollbar. */
export function TextEditor(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const ref = useAutoHeight(props.value ?? props.placeholder);
  return <textarea ref={ref} spellCheck {...props} className={["editor", props.className].filter(Boolean).join(" ")} />;
}
