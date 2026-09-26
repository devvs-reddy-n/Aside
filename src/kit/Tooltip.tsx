import * as T from "@radix-ui/react-tooltip";
import type { ReactElement, ReactNode } from "react";

/** Mount once per window. */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return <T.Provider delayDuration={450} skipDelayDuration={250}>{children}</T.Provider>;
}

/** A small label that appears on hover or keyboard focus. */
export function Tip({ label, side = "top", children }: { label: ReactNode; side?: "top" | "bottom" | "left" | "right"; children: ReactElement }) {
  if (!label) return children;
  return (
    <T.Root>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content className="tip" side={side} sideOffset={6} collisionPadding={8}>{label}</T.Content>
      </T.Portal>
    </T.Root>
  );
}
