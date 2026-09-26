import { useStore } from "../core/store";
import { Icon } from "../kit/Icon";
import { Popover } from "../kit/Popover";
import { systemStore } from "./data";
import { engineName, modelLabel } from "./labels";
import { aiProblem, speechProblem } from "./readiness";
import { hrefFor } from "./router";

interface StatusRow {
  label: string;
  state: string;
  tone: "ok" | "warn" | "off";
  fix?: { label: string; href: string };
}

function rowsFor(system: ReturnType<typeof systemStore.get>): StatusRow[] {
  const speech = speechProblem(system);
  const ai = aiProblem(system);
  return [
    speech
      ? { label: "Speech", state: speech, tone: "warn", fix: { label: "Fix", href: hrefFor("settings", "speech") } }
      : { label: "Speech", state: engineName(system.engine!), tone: "ok" },
    ai
      ? { label: "AI model", state: ai, tone: "warn", fix: { label: "Fix", href: hrefFor("settings", "ai") } }
      : { label: "AI model", state: modelLabel(system.provider!, system.model!).replace(/^[^·]+· /, ""), tone: "ok" },
    { label: "Microphone", state: "Checked when a session starts", tone: "off" },
    { label: "System audio", state: "Checked when a session starts", tone: "off" },
    system.hidden === false
      ? { label: "Screen sharing", state: "Aside is visible", tone: "warn", fix: { label: "Change", href: hrefFor("settings", "general") } }
      : { label: "Screen sharing", state: system.hidden ? "Hidden" : "Unknown", tone: system.hidden ? "ok" : "off" },
  ];
}

/** One pill that says whether Aside is ready; the details open in a small popover. */
export function StatusPill({ compact }: { compact?: boolean }) {
  const system = useStore(systemStore, (state) => state);
  const rows = rowsFor(system);
  const attention = system.loaded && rows.some((row) => row.tone === "warn");
  const label = !system.loaded ? "Checking…" : attention ? "Needs attention" : "Ready";

  return (
    <Popover
      side={compact ? "right" : "top"}
      trigger={
        <button type="button" className="status-pill" data-tone={!system.loaded ? "off" : attention ? "warn" : "ok"} aria-label={`Status: ${label}`}>
          <i aria-hidden="true" />
          {compact ? null : <span>{label}</span>}
        </button>
      }
    >
      <div className="status-pop">
        <p className="status-pop__title">{label}</p>
        <ul>
          {rows.map((row) => (
            <li key={row.label}>
              <span className="dot" data-tone={row.tone} aria-hidden="true" />
              <span className="status-pop__text"><b>{row.label}</b><small>{row.state}</small></span>
              {row.fix ? <a className="status-pop__fix" href={row.fix.href}>{row.fix.label}<Icon name="chevronRight" size={13} /></a> : null}
            </li>
          ))}
        </ul>
      </div>
    </Popover>
  );
}
