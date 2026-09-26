import { useState } from "react";
import * as api from "../../bridge/api";
import type { CopilotSummary } from "../../bridge/types";
import { openPrivacySettings } from "../../bridge/system";
import { plural } from "../../core/format";
import { useStore } from "../../core/store";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Icon } from "../../kit/Icon";
import { goLive, prepareCall, systemStore, type ChannelFailure, type LaunchStep, type PreparedCall } from "../data";
import { blockers, checksFor, type Check } from "../readiness";
import { hrefFor } from "../router";
import { AudioCheck } from "./AudioCheck";

type Launch =
  | { kind: "idle" }
  | { kind: "running"; step: LaunchStep }
  | { kind: "trouble"; call: PreparedCall }
  | { kind: "error"; message: string };

const CHANNEL_NAMES: Record<ChannelFailure["channel"], string> = { mic: "Microphone", system: "Other side" };

const STEPS: Array<{ id: LaunchStep; label: string }> = [
  { id: "session", label: "Setup" },
  { id: "mic", label: "Microphone" },
  { id: "system", label: "Other side" },
  { id: "window", label: "Live window" },
];

/** One short word per check: a tick when fine, an amber pill (with a fix link) when it needs attention. */
function pillText(check: Check): string {
  switch (check.id) {
    case "speech": return "Speech";
    case "ai": return "AI model";
    case "sources": return check.status === "ok" ? "Resume" : "No resume";
    case "privacy": return check.status === "ok" ? "Hidden" : "Visible";
    default: return check.title;
  }
}

function Checks({ checks }: { checks: Check[] }) {
  return (
    <ul className="lp-pills">
      {checks.map((check) => {
        const body = (
          <>
            <Icon name={check.status === "ok" ? "check" : check.status === "todo" ? "alert" : "info"} size={13} />
            {pillText(check)}
          </>
        );
        return (
          <li key={check.id}>
            {check.fix ? (
              <a className="lp-pill" data-status={check.status} href={hrefFor(check.fix.page, check.fix.param)} title={`${check.detail} ${check.fix.label}`}>{body}</a>
            ) : (
              <span className="lp-pill" data-status={check.status} title={check.detail}>{body}</span>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function Steps({ current }: { current: LaunchStep }) {
  const at = STEPS.findIndex((step) => step.id === current);
  return (
    <ol className="lp-steps" aria-label="Starting your session" aria-live="polite">
      {STEPS.map((step, index) => (
        <li key={step.id} data-state={index < at ? "done" : index === at ? "now" : "next"}>
          <span aria-hidden="true">{index < at ? <Icon name="check" size={12} /> : index === at ? <i /> : null}</span>
          {step.label}
        </li>
      ))}
    </ol>
  );
}

/** A small window over the workspace: quick checks, an optional audio test, then Start. Both sides always start; either can be turned off in the live window. */
export function StartDialog({ copilot, onClose }: { copilot: CopilotSummary; onClose: () => void }) {
  const system = useStore(systemStore, (state) => state);
  const [launch, setLaunch] = useState<Launch>({ kind: "idle" });
  const [testing, setTesting] = useState(false);

  const checks = checksFor(system, copilot).filter((check) => check.id !== "permissions");
  const problems = blockers(checks).length;
  const active = system.active;
  const running = launch.kind === "running";

  async function start() {
    if (running || active || testing) return;
    setLaunch({ kind: "running", step: "session" });
    const result = await prepareCall(copilot.id, { onStep: (step) => setLaunch({ kind: "running", step }) });
    if (!result.ok) {
      setLaunch({ kind: "error", message: result.error });
      return;
    }
    if (result.value.failures.length > 0) {
      setLaunch({ kind: "trouble", call: result.value });
      return;
    }
    await openLive();
  }

  async function openLive() {
    setLaunch({ kind: "running", step: "window" });
    const live = await goLive();
    if (live.ok) onClose();
    else setLaunch({ kind: "error", message: live.error });
  }

  async function cancelTrouble(call: PreparedCall) {
    await api.live.end(call.session.id);
    onClose();
  }

  async function tryAgain(call: PreparedCall) {
    await api.live.end(call.session.id);
    await start();
  }

  const title = copilot.company || copilot.name;
  const status =
    launch.kind === "running" ? <Steps current={launch.step} />
    : launch.kind === "error" ? <span className="lp-bad"><Icon name="alert" size={15} />{launch.message}</span>
    : active ? <span>A call is running.</span>
    : testing ? <span>Testing audio…</span>
    : problems > 0 ? <span className="lp-warn">{plural(problems, "thing")} to fix</span>
    : null;

  return (
    <Dialog
      open
      onClose={() => { if (!running) onClose(); }}
      size="md"
      title="Start session"
      subtitle={[title, copilot.target_role].filter(Boolean).join(" · ")}
      footer={
        launch.kind === "trouble" ? (
          <>
            <Button variant="ghost" onClick={() => void cancelTrouble(launch.call)}>Cancel</Button>
            <Button variant="secondary" icon="refresh" onClick={() => void tryAgain(launch.call)}>Retry</Button>
            <Button variant="primary" iconRight="arrow" onClick={() => void openLive()}>Start anyway</Button>
          </>
        ) : (
          <>
            <span className="sd-status">{status}</span>
            <Button variant="ghost" disabled={running} onClick={onClose}>Cancel</Button>
            <Button variant="primary" iconRight="arrow" busy={running} disabled={Boolean(active) || testing} onClick={() => void start()}>
              {running ? "Starting" : problems > 0 ? "Start anyway" : "Start"}
            </Button>
          </>
        )
      }
    >
      <div className="sd">
        {launch.kind === "trouble" ? (
          <section className="lp-trouble" role="alert" aria-labelledby="lp-trouble-title">
            <h3 id="lp-trouble-title"><Icon name="alert" size={17} />Audio didn't start</h3>
            <ul>
              {launch.call.failures.map((failure) => (
                <li key={failure.channel}>
                  <div><b>{CHANNEL_NAMES[failure.channel]}</b><p>{failure.message}</p></div>
                  <button type="button" className="lp-fix" onClick={() => void openPrivacySettings(failure.channel === "mic" ? "microphone" : "screen")}>Settings</button>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        <Checks checks={checks} />
        <div>
          <h3 className="sd-h">Test audio <small>optional</small></h3>
          <AudioCheck disabled={running || Boolean(active)} onBusy={setTesting} />
        </div>
      </div>
    </Dialog>
  );
}
