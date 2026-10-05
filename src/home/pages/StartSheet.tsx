import { useEffect, useRef, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import type { ChannelId, CopilotSummary } from "../../bridge/types";
import { openPrivacySettings } from "../../bridge/system";
import { useStore } from "../../core/store";
import { Button } from "../../kit/Button";
import { Icon } from "../../kit/Icon";
import { Sheet } from "../../kit/Sheet";
import { goLive, prepareCall, systemStore, type LaunchStep, type PreparedCall } from "../data";
import { APP_SHORTCUTS } from "../../core/shortcuts";
import { copilotModelLabel, engineName } from "../labels";
import { aiProblem, speechProblem } from "../readiness";
import { hrefFor } from "../router";
import { LevelBars } from "../../kit/LevelBars";
import { probeChannel, type ProbeResult } from "./AudioCheck";

type Tone = "waiting" | "checking" | "ok" | "info" | "warn" | "bad";
interface CheckRow { id: string; label: string; tone: Tone; detail: string; action?: ReactNode }

type Launch =
  | { kind: "idle" }
  | { kind: "running"; step: LaunchStep }
  | { kind: "trouble"; call: PreparedCall }
  | { kind: "error"; message: string };

const STEP_TEXT: Record<LaunchStep, string> = { session: "Getting ready", mic: "Starting your microphone", system: "Starting system audio", window: "Opening the live window" };
const STAGGER_MS = 80;

function ToneIcon({ tone }: { tone: Tone }) {
  if (tone === "checking") return <span className="pf-spin" aria-hidden="true" />;
  if (tone === "waiting") return <span className="pf-wait" aria-hidden="true" />;
  return <Icon name={tone === "ok" ? "check" : tone === "info" ? "info" : "alert"} size={14} />;
}

function audioRow(channel: ChannelId, result: ProbeResult | "checking" | null, retry: () => void): CheckRow {
  const mic = channel === "mic";
  const pane = mic ? "microphone" : "screen";
  const label = mic ? "Microphone" : "System audio";
  if (result === null) return { id: channel, label, tone: "waiting", detail: "Waiting" };
  if (result === "checking") return { id: channel, label, tone: "checking", detail: mic ? "Listening… say something" : "Listening…" };
  if (result.kind === "heard") return { id: channel, label, tone: "ok", detail: result.device ? `Working · ${result.device}` : "Working" };
  if (result.kind === "failed") {
    return {
      id: channel,
      label: mic ? "Allow microphone" : "Allow system audio recording",
      tone: "bad",
      detail: mic ? "Aside needs the microphone to hear you." : "Aside needs Screen & System Audio Recording to hear the other side.",
      action: <Button size="sm" variant="secondary" onClick={() => void openPrivacySettings(pane)}>Open System Settings</Button>,
    };
  }
  // Connected but silent. Normal for system audio before a call; a real hint for the microphone.
  return mic
    ? { id: channel, label, tone: "warn", detail: "Nothing heard. Check your input device, then retry.", action: <Button size="sm" variant="ghost" icon="refresh" onClick={retry}>Retry</Button> }
    : { id: channel, label, tone: "info", detail: "Connected. Nothing is playing yet, which is normal before a call.", action: <Button size="sm" variant="ghost" onClick={() => void openPrivacySettings(pane)}>Not working?</Button> };
}

/** A small panel before a call: every check runs by itself, one after another, then Start. */
export function StartSheet({ copilot, onClose }: { copilot: CopilotSummary; onClose: () => void }) {
  const system = useStore(systemStore, (state) => state);
  const [revealed, setRevealed] = useState(0);
  const [probes, setProbes] = useState<Record<ChannelId, ProbeResult | "checking" | null>>({ mic: null, system: null });
  const [levels, setLevels] = useState<Record<ChannelId, number>>({ mic: 0, system: 0 });
  const [launch, setLaunch] = useState<Launch>({ kind: "idle" });
  const abort = useRef<AbortController | null>(null);
  const probing = useRef<Promise<void> | null>(null);

  function runProbes(channels: ChannelId[]): Promise<void> {
    if (!probing.current) probing.current = probe(channels).finally(() => { probing.current = null; });
    return probing.current;
  }

  async function probe(channels: ChannelId[]) {
    const controller = new AbortController();
    abort.current = controller;
    for (const channel of channels) {
      if (controller.signal.aborted) break;
      setProbes((current) => ({ ...current, [channel]: "checking" }));
      const result = await probeChannel(channel, { signal: controller.signal, onLevel: (rms) => setLevels((current) => ({ ...current, [channel]: rms })) });
      if (controller.signal.aborted) break;
      setProbes((current) => ({ ...current, [channel]: result }));
    }
  }

  // The quick checks appear one by one, then each side of the audio is tried in turn.
  useEffect(() => {
    const timers = [1, 2, 3].map((count) => window.setTimeout(() => setRevealed(count), count * STAGGER_MS));
    const start = window.setTimeout(() => void runProbes(["mic", "system"]), 3 * STAGGER_MS + 120);
    return () => {
      timers.forEach(window.clearTimeout);
      window.clearTimeout(start);
      abort.current?.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Back from System Settings: try the sides that didn't work again.
  useEffect(() => {
    const onFocus = () => {
      const failed = (["mic", "system"] as ChannelId[]).filter((channel) => {
        const probe = probes[channel];
        return probe !== null && probe !== "checking" && probe.kind === "failed";
      });
      if (failed.length > 0 && launch.kind === "idle") void runProbes(failed);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  const speech = speechProblem(system);
  const ai = aiProblem(system);
  const staticRows: CheckRow[] = [
    speech
      ? { id: "speech", label: "Speech", tone: "bad", detail: speech, action: <a className="pf-fix" onClick={onClose} href={hrefFor("settings", "speech")}>Set up</a> }
      : { id: "speech", label: "Speech", tone: "ok", detail: `${engineName(system.engine!)} is ready` },
    ai
      ? { id: "ai", label: "AI model", tone: "bad", detail: ai, action: <a className="pf-fix" onClick={onClose} href={hrefFor("settings", "ai")}>Set up</a> }
      : { id: "ai", label: "AI model", tone: "ok", detail: copilotModelLabel(copilot.llm_provider, copilot.llm_model, system) },
    copilot.has_resume
      ? { id: "resume", label: "Resume", tone: "ok", detail: copilot.document_count > 1 ? `Attached, plus ${copilot.document_count - 1} more file${copilot.document_count === 2 ? "" : "s"}` : "Attached" }
      : { id: "resume", label: "Resume", tone: "info", detail: "None attached. Answers will be more general." },
  ];
  const rows: CheckRow[] = [
    ...staticRows.map((row, index) => (index < revealed ? row : { ...row, tone: "waiting" as Tone, detail: "Waiting", action: undefined })),
    audioRow("mic", probes.mic, () => void runProbes(["mic"])),
    audioRow("system", probes.system, () => void runProbes(["system"])),
    ...(system.hidden === false ? [{ id: "privacy", label: "Screen sharing", tone: "warn" as Tone, detail: "Aside is visible to people you share your screen with.", action: <a className="pf-fix" onClick={onClose} href={hrefFor("settings", "general")}>Change</a> }] : []),
  ];

  const checking = rows.some((row) => row.tone === "waiting" || row.tone === "checking");
  const allGood = !checking && rows.every((row) => row.tone === "ok" || row.tone === "info");
  const running = launch.kind === "running";
  const active = system.active;

  async function start() {
    if (running || active) return;
    // A test still listening must fully let go of the audio before the call takes it.
    abort.current?.abort();
    await probing.current;
    setLaunch({ kind: "running", step: "session" });
    const result = await prepareCall(copilot.id, { onStep: (step) => setLaunch({ kind: "running", step }) });
    if (!result.ok) return setLaunch({ kind: "error", message: result.error });
    if (result.value.failures.length > 0) return setLaunch({ kind: "trouble", call: result.value });
    await openLive();
  }

  async function openLive() {
    setLaunch({ kind: "running", step: "window" });
    const live = await goLive();
    if (live.ok) onClose();
    else setLaunch({ kind: "error", message: live.error });
  }

  async function endPrepared(call: PreparedCall, then: () => void | Promise<void>) {
    await api.live.end(call.session.id);
    await then();
  }

  const footer =
    launch.kind === "trouble" ? (
      <>
        <Button variant="ghost" onClick={() => void endPrepared(launch.call, onClose)}>Cancel</Button>
        <span className="grow" />
        <Button variant="secondary" icon="refresh" onClick={() => void endPrepared(launch.call, start)}>Retry</Button>
        <Button variant="primary" iconRight="arrow" onClick={() => void openLive()}>Start anyway</Button>
      </>
    ) : (
      <>
        <span className="pf-status" aria-live="polite">
          {running ? <><span className="pf-spin" aria-hidden="true" />{STEP_TEXT[launch.step]}…</>
            : launch.kind === "error" ? <span className="pf-bad">{launch.message}</span>
            : active ? "A call is already running."
            : checking ? "Checking…" : null}
        </span>
        <Button variant="ghost" disabled={running} onClick={onClose}>Cancel</Button>
        <Button variant="primary" iconRight="arrow" busy={running} disabled={Boolean(active) || (checking && !running)} onClick={() => void start()}>
          {allGood || checking || running ? "Start" : "Start anyway"}
        </Button>
      </>
    );

  return (
    <Sheet open onClose={() => { if (!running) onClose(); }} title="Start session" meta={[copilot.company || copilot.name, copilot.target_role].filter(Boolean).join(" · ")} width={460} footer={footer}>
      {launch.kind === "trouble" ? (
        <div className="callout callout--warn pf-trouble" role="alert">
          <b>Audio didn't start on {launch.call.failures.map((f) => (f.channel === "mic" ? "your microphone" : "system audio")).join(" or ")}.</b>
          <p>You can start anyway and turn it on from the live window, or fix it and retry.</p>
        </div>
      ) : null}
      <ul className="pf" aria-label="Checks before the call">
        {rows.map((row) => (
          <li key={row.id} className="pf-row" data-tone={row.tone}>
            <span className="pf-row__icon"><ToneIcon tone={row.tone} /></span>
            <span className="pf-row__text">
              <b>{row.label}</b>
              <small key={row.detail} className="pf-fade">{row.detail}</small>
            </span>
            {row.id === "mic" || row.id === "system" ? <LevelBars level={levels[row.id as ChannelId]} active={row.tone === "checking"} /> : null}
            {row.action ? <span className="pf-row__action">{row.action}</span> : null}
          </li>
        ))}
      </ul>
      <p className="pf-note">Both sides start listening. You can turn either off in the live window.</p>
      <section className="pf-keys" aria-label="Shortcuts during the call">
        <b>Shortcuts</b>
        <dl>{APP_SHORTCUTS.map(([keys, what]) => <div key={keys}><dt>{what}</dt><dd>{keys}</dd></div>)}</dl>
      </section>
    </Sheet>
  );
}
