import { displayFileName } from "../../core/text";
import { audioPermissionHelp, systemSettingsLabel } from "../../core/platform";
import { useEffect, useRef, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import type { ChannelId, CopilotSummary, StoredDocument } from "../../bridge/types";
import { openPrivacySettings } from "../../bridge/system";
import { useStore } from "../../core/store";
import { Button } from "../../kit/Button";
import { Icon } from "../../kit/Icon";
import { Sheet } from "../../kit/Sheet";
import { goLive, prepareCall, systemStore, type LaunchStep, type PreparedCall } from "../data";
import { APP_SHORTCUTS } from "../../core/shortcuts";
import { copilotModelLabel, DEFAULT_MODEL_CHOICE, engineName } from "../labels";
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
  if (result === "checking") return { id: channel, label, tone: "checking", detail: mic ? "Listening… say something" : "Playing a short test sound…" };
  if (result.kind === "cancelled") return { id: channel, label, tone: "info", detail: "Check cancelled" };
  if (result.kind === "heard") return { id: channel, label, tone: "ok", detail: result.device ? `${mic ? "Working" : "Sound detected"} · ${result.device}` : "Working" };
  if (result.kind === "failed") {
    return {
      id: channel,
      label,
      tone: "bad",
      detail: /denied|not authorized|not permitted|tcc|permission.*(?:needed|required|missing)/i.test(result.message) ? audioPermissionHelp(mic ? "mic" : "system") : result.message,
      action: <div className="pf-audio-actions"><Button size="sm" variant="ghost" icon="refresh" onClick={retry}>Retry</Button><Button size="sm" variant="secondary" onClick={() => void openPrivacySettings(pane)}>{systemSettingsLabel}</Button></div>,
    };
  }
  // Silence after a test is inconclusive: offer a retry instead of a green check.
  return mic
    ? { id: channel, label, tone: "warn", detail: "Nothing heard. Check your input device, then retry.", action: <Button size="sm" variant="ghost" icon="refresh" onClick={retry}>Retry</Button> }
    : { id: channel, label, tone: "warn", detail: result.soundError ?? "The test sound wasn’t detected. Check your output volume and device, then retry.", action: <Button size="sm" variant="ghost" icon="refresh" onClick={retry}>Retry</Button> };
}

type StartSheetProps = { onClose: () => void } & (
  | { copilot: CopilotSummary; resume?: never }
  | { resume: StoredDocument; copilot?: never }
);

/** Shared checks for a saved Copilot or a quick call with a Library resume. */
export function StartSheet({ copilot, resume, onClose }: StartSheetProps) {
  const system = useStore(systemStore, (state) => state);
  const [probes, setProbes] = useState<Record<ChannelId, ProbeResult | "checking" | null>>({ mic: null, system: null });
  const [levels, setLevels] = useState<Record<ChannelId, number>>({ mic: 0, system: 0 });
  const [launch, setLaunch] = useState<Launch>({ kind: "idle" });
  const abort = useRef<AbortController | null>(null);
  const probing = useRef<Promise<void> | null>(null);

  function runProbes(channels: ChannelId[]): Promise<void> {
    if (system.active) return Promise.resolve();
    if (!probing.current) probing.current = probe(channels).finally(() => { probing.current = null; });
    return probing.current;
  }

  async function probe(channels: ChannelId[]) {
    const controller = new AbortController();
    abort.current = controller;
    // Bluetooth inputs can switch the shared output into call mode. Finish and
    // release each check before opening the next, so a tap never races that switch.
    for (const channel of channels) {
      if (controller.signal.aborted) break;
      setProbes((current) => ({ ...current, [channel]: "checking" }));
      let lastLevel = 0;
      const result = await probeChannel(channel, { signal: controller.signal, onLevel: (rms) => {
        // Native meters can send hundreds of events a second; paint at most ten.
        const now = performance.now();
        if (rms !== 0 && now - lastLevel < 100) return;
        lastLevel = now;
        if (!controller.signal.aborted) setLevels((current) => ({ ...current, [channel]: rms }));
      } });
      if (!controller.signal.aborted) setProbes((current) => ({ ...current, [channel]: result }));
    }
  }

  // Known checks appear immediately; no decorative delay before testing devices.
  useEffect(() => {
    const start = window.setTimeout(() => void runProbes(["system", "mic"]), 0);
    return () => { window.clearTimeout(start); abort.current?.abort(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Retry is explicit: focus changes must not replay tones or restart a driver
  // while an earlier timed-out native worker is still unwinding.

  const speech = speechProblem(system);
  const ai = aiProblem(system);
  const staticRows: CheckRow[] = [
    speech
      ? { id: "speech", label: "Speech", tone: "bad", detail: speech, action: <a className="pf-fix" onClick={onClose} href={hrefFor("settings", "speech")}>Set up</a> }
      : { id: "speech", label: "Speech", tone: "ok", detail: `${engineName(system.engine!)} is ready` },
    ai
      ? { id: "ai", label: "AI model", tone: "bad", detail: ai, action: <a className="pf-fix" onClick={onClose} href={hrefFor("settings", "ai")}>Set up</a> }
      : { id: "ai", label: "AI model", tone: "ok", detail: copilotModelLabel(copilot?.llm_provider ?? system.provider ?? "open_ai", copilot?.llm_model ?? DEFAULT_MODEL_CHOICE, system) },
    (resume || copilot?.has_resume)
      ? { id: "resume", label: "Resume", tone: "ok", detail: resume ? displayFileName(resume.file_name) : copilot && copilot.document_count > 1 ? `Attached, plus ${copilot.document_count - 1} more file${copilot.document_count === 2 ? "" : "s"}` : "Attached" }
      : { id: "resume", label: "Resume", tone: "info", detail: "None attached. Answers will be more general." },
  ];
  const rows: CheckRow[] = [
    ...staticRows,
    audioRow("system", probes.system, () => void runProbes(["system"])),
    audioRow("mic", probes.mic, () => void runProbes(["mic"])),
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
    const result = await prepareCall(resume ? { resumeId: resume.id } : copilot.id, { onStep: (step) => setLaunch({ kind: "running", step }) });
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
    <Sheet open onClose={() => { if (!running) onClose(); }} title={resume ? "Start quick call" : "Start session"} meta={resume ? "Your resume + default model. No job description needed." : [copilot.company || copilot.name, copilot.target_role].filter(Boolean).join(" · ")} width={460} footer={footer}>
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
      <p className="pf-note">Checks stay local. A short sound tests system audio; nothing is transcribed.{resume ? " Transcript saved in History." : ""}</p>
      <section className="pf-keys" aria-label="Shortcuts during the call">
        <b>Shortcuts</b>
        <dl>{APP_SHORTCUTS.map(([keys, what]) => <div key={keys}><dt>{what}</dt><dd>{keys}</dd></div>)}</dl>
      </section>
    </Sheet>
  );
}
