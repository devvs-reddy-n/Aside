import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export function Spinner({ size = 16 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} role="status" aria-label="Loading" />;
}

export function Badge({ tone = "neutral", children }: { tone?: "neutral" | "ok" | "warn" | "bad" | "blue"; children: ReactNode }) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function StatusDot({ tone }: { tone: "ok" | "warn" | "bad" | "off" }) {
  return <span className={`dot dot--${tone}`} aria-hidden="true" />;
}

export function EmptyState({ icon, title, children, action }: { icon: IconName; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <span className="empty__mark"><Icon name={icon} size={22} /></span>
      <h2>{title}</h2>
      {children ? <p>{children}</p> : null}
      {action}
    </div>
  );
}

/** A soft inline message: tone decides colour and icon. */
export function Callout({ tone = "info", title, children, actions }: { tone?: "info" | "warn" | "bad" | "ok"; title?: string; children?: ReactNode; actions?: ReactNode }) {
  const icon: IconName = tone === "ok" ? "check" : tone === "info" ? "info" : "alert";
  return (
    <div className={`callout callout--${tone}`} role={tone === "bad" ? "alert" : "status"}>
      <Icon name={icon} size={18} />
      <div className="callout__body">
        {title ? <strong>{title}</strong> : null}
        {children ? <p>{children}</p> : null}
        {actions ? <div className="callout__actions">{actions}</div> : null}
      </div>
    </div>
  );
}

const FILE_TILES: Record<string, { label: string; kind: string }> = {
  Word: { label: "W", kind: "word" },
  PDF: { label: "PDF", kind: "pdf" },
  Text: { label: "TXT", kind: "text" },
};

/** A small coloured tile that says what kind of file this is (Word blue, PDF red, text grey). */
export function FileTile({ kind }: { kind: string }) {
  const tile = FILE_TILES[kind] ?? { label: kind.slice(0, 4), kind: "other" };
  return <span className="file-tile" data-kind={tile.kind} aria-hidden="true">{tile.label}</span>;
}

export interface Meta {
  icon?: IconName;
  label: string | null | undefined | false;
  /** Read aloud before the value, e.g. "Duration". */
  name?: string;
}

/** Compact facts as chips (role, company, date, duration…). Only the ones that exist are shown. */
export function MetaChips({ items, className }: { items: Meta[]; className?: string }) {
  const shown = items.filter((item): item is Meta & { label: string } => Boolean(item.label && String(item.label).trim()));
  if (shown.length === 0) return null;
  return (
    <ul className={["chips", className].filter(Boolean).join(" ")}>
      {shown.map((item) => (
        <li key={`${item.name ?? ""}${item.label}`} className="chip" aria-label={item.name ? `${item.name}: ${item.label}` : undefined}>
          {item.icon ? <Icon name={item.icon} size={12} strokeWidth={2} /> : null}
          <span>{item.label}</span>
        </li>
      ))}
    </ul>
  );
}
