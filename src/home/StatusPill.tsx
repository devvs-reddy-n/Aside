import type { ModelLink } from "../bridge/types";
import { useStore } from "../core/store";
import { Icon } from "../kit/Icon";
import { Popover } from "../kit/Popover";
import { systemStore } from "./data";
import { engineName, modelLabel } from "./labels";
import { aiProblem, speechProblem } from "./readiness";
import { hrefFor } from "./router";

type Tone = "ok" | "bad" | "warn" | "off" | "checking";

interface StatusRow {
  label: string;
  state: string;
  tone: Tone;
  fix?: { label: string; href: string };
}

type System = ReturnType<typeof systemStore.get>;

/** Green when Aside reached it, red when it couldn't, grey while checking or when no check exists. */
function linkTone(link: ModelLink | undefined): Tone {
  if (!link) return "checking";
  return link.state === "connected" ? "ok" : link.state === "failed" ? "bad" : "off";
}

function aiName(system: System): string {
  return system.provider && system.model ? modelLabel(system.provider, system.model).replace(/^[^·]+· /, "") : "No AI model";
}

function speechName(system: System): string {
  return system.engine ? engineName(system.engine) : "No speech engine";
}

function rowsFor(system: System): StatusRow[] {
  const speechLink = system.links?.speech;
  const aiLink = system.links?.ai;
  return [
    {
      label: `Speech · ${speechName(system)}`,
      state: speechLink?.detail ?? speechProblem(system) ?? "Checking…",
      tone: linkTone(speechLink),
      fix: speechLink?.state === "failed" ? { label: "Fix", href: hrefFor("settings", "speech") } : undefined,
    },
    {
      label: `AI model · ${aiName(system)}`,
      state: aiLink?.detail ?? aiProblem(system) ?? "Checking…",
      tone: linkTone(aiLink),
      fix: aiLink?.state === "failed" ? { label: "Fix", href: hrefFor("settings", "ai") } : undefined,
    },
    { label: "Microphone", state: "Checked when a session starts", tone: "off" },
    { label: "System audio", state: "Checked when a session starts", tone: "off" },
    system.hidden === false
      ? { label: "Screen sharing", state: "Aside is visible", tone: "warn", fix: { label: "Change", href: hrefFor("settings", "general") } }
      : { label: "Screen sharing", state: system.hidden ? "Hidden" : "Unknown", tone: system.hidden ? "ok" : "off" },
  ];
}

/**
 * The default AI model and speech engine, each with a dot that says whether
 * Aside could actually reach it. More detail opens in a small popover.
 */
export function StatusPill({ compact }: { compact?: boolean }) {
  const system = useStore(systemStore, (state) => state);
  const rows = rowsFor(system);
  const ai = linkTone(system.links?.ai);
  const speech = linkTone(system.links?.speech);
  const summary = `AI model ${aiName(system)}: ${rows[1].state} Speech ${speechName(system)}: ${rows[0].state}`;

  return (
    <Popover
      side={compact ? "right" : "top"}
      trigger={
        <button type="button" className="status-pill status-pill--models" aria-label={summary}>
          <span className="sp-line"><i className="sp-dot" data-tone={ai} aria-hidden="true" />{compact ? null : <span>{aiName(system)}</span>}</span>
          <span className="sp-line"><i className="sp-dot" data-tone={speech} aria-hidden="true" />{compact ? null : <span>{speechName(system)}</span>}</span>
        </button>
      }
    >
      <div className="status-pop">
        <p className="status-pop__title">Connections</p>
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
