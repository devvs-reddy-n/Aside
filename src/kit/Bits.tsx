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
