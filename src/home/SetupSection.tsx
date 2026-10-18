import type { ReactNode, TextareaHTMLAttributes } from "react";
import { Icon, type IconName } from "../kit/Icon";
import { Popover } from "../kit/Popover";

/** Setup sections share the same label, action and field alignment. */
export function SetupSection({ name, title, icon, helper, action, children }: {
  name: string; title: string; icon: IconName; helper?: ReactNode; action?: ReactNode; children: ReactNode;
}) {
  return <section className={`cp-tile cp-tile--${name}`} aria-label={title}>
    <header className="cp-tile__head">
      <div className="cp-tile__label"><Icon name={icon} size={15} /><h2>{title}</h2>{helper ? (
        <Popover side="bottom" trigger={<button type="button" className="cp-info" aria-label={`About ${title}`}><Icon name="info" size={14} /></button>}>
          <div className="cp-help">{helper}</div>
        </Popover>
      ) : null}</div>
      {action ? <div className="setup-actions">{action}</div> : null}
    </header>
    <div className="cp-tile__content">{children}</div>
  </section>;
}

/** A full-width preview doubles as a generous target for the complete editor. */
export function SetupPreview({ text, empty, label, onClick, disabled = false }: {
  text: string | null | undefined; empty: string; label: string; onClick: () => void; disabled?: boolean;
}) {
  const value = text?.replace(/\s+/g, " ").trim();
  return <button type="button" className="setup-field setup-preview" data-empty={!value || undefined}
    aria-label={label} aria-haspopup="dialog" disabled={disabled} onClick={onClick}>
    <span className="setup-preview__text">{value || empty}</span><Icon name="chevronRight" size={15} />
  </button>;
}

/** Text fills the available setup space; only the text itself scrolls. */
export function SetupEditor(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea spellCheck {...props} className={["setup-editor", props.className].filter(Boolean).join(" ")} />;
}
