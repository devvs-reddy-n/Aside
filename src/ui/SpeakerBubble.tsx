import type { HTMLAttributes, ReactNode } from "react";

export type Speaker = "me" | "them";

export interface SpeakerBubbleProps extends HTMLAttributes<HTMLDivElement> {
  speaker: Speaker;
  text: ReactNode;
  timestamp?: ReactNode;
  className?: string;
}

/** Two-speaker chat bubble. Existing geometry kept (16px 16px 16px 4px,
 *  mirrored). See DESIGN.md §8 "Speaker bubble". */
export function SpeakerBubble({
  speaker,
  text,
  timestamp,
  className,
  ...rest
}: SpeakerBubbleProps) {
  const classes = ["u-bubble", `u-bubble--${speaker}`, className]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} {...rest}>
      <p className="u-bubble__text">{text}</p>
      {timestamp ? <footer className="u-bubble__meta">{timestamp}</footer> : null}
    </div>
  );
}
