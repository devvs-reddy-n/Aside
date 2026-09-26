import * as P from "@radix-ui/react-popover";
import type { ReactElement, ReactNode } from "react";

export function Popover({ trigger, children, side = "top", align = "start", open, onOpenChange }: {
  trigger: ReactElement; children: ReactNode; side?: "top" | "bottom" | "left" | "right"; align?: "start" | "center" | "end";
  open?: boolean; onOpenChange?: (open: boolean) => void;
}) {
  return (
    <P.Root open={open} onOpenChange={onOpenChange}>
      <P.Trigger asChild>{trigger}</P.Trigger>
      <P.Portal>
        <P.Content className="pop" side={side} align={align} sideOffset={8} collisionPadding={12}>{children}</P.Content>
      </P.Portal>
    </P.Root>
  );
}
