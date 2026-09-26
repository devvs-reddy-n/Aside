import * as M from "@radix-ui/react-dropdown-menu";
import type { ReactElement } from "react";
import { Icon, type IconName } from "./Icon";

export interface MenuItem {
  label: string;
  icon?: IconName;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

/** A small menu that opens from its trigger (for example a ⋯ button). */
export function Menu({ trigger, items, align = "end" }: { trigger: ReactElement; items: Array<MenuItem | "separator">; align?: "start" | "end" }) {
  return (
    <M.Root modal={false}>
      <M.Trigger asChild>{trigger}</M.Trigger>
      <M.Portal>
        <M.Content className="menu" align={align} sideOffset={6} collisionPadding={8}>
          {items.map((item, index) =>
            item === "separator" ? (
              <M.Separator key={index} className="menu__sep" />
            ) : (
              <M.Item key={item.label} className="menu__item" data-danger={item.danger || undefined} disabled={item.disabled} onSelect={item.onSelect}>
                {item.icon ? <Icon name={item.icon} size={15} /> : null}
                {item.label}
              </M.Item>
            ),
          )}
        </M.Content>
      </M.Portal>
    </M.Root>
  );
}
