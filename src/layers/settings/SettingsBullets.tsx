import type { ReactNode } from "react";

export function SettingsBullets({ items }: { items: ReactNode[] }) {
  return (
    <ul className="settings-bullets">
      {items.map((item, index) => (
        <li key={index}>{item}</li>
      ))}
    </ul>
  );
}
