import { useEffect, useRef, useState } from "react";
import * as api from "../../bridge/api";
import { onEvent } from "../../bridge/events";
import { channelEventNames } from "../../bridge/eventNames";
import { isTauriRuntime } from "../../bridge/runtime";
import { openPrivacySettings } from "../../bridge/system";
import type { ChannelId } from "../../bridge/types";
import { Icon } from "../../kit/Icon";

const TEST_MS = 4500;
/** Loudness above which a side is proven to be hearing something (same bar the live window uses). */
const HEARD = 0.002;

type Phase =
  | { kind: "idle" }
  | { kind: "testing" }
  | { kind: "heard"; device: string | null }
  | { kind: "quiet" }
  | { kind: "failed"; message: string };

interface Side {
  channel: ChannelId;
  name: string;
  icon: "mic" | "speaker";
  pane: "microphone" | "screen";
}

const SIDES: Side[] = [
  { channel: "mic", name: "Microphone", icon: "mic", pane: "microphone" },
  { channel: "system", name: "Other side", icon: "speaker", pane: "screen" },
];

function Row({ side, disabled, onBusy }: { side: Side; disabled: boolean; onBusy: (busy: boolean) => void }) {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [level, setLevel] = useState(0);
  const running = useRef(false);
  const stopNow = useRef<() => void>(() => undefined);

  // Leaving the page mid-test must let go of the audio.
  useEffect(() => () => stopNow.current(), []);

  async function test() {
    if (running.current) return;
    running.current = true;
    onBusy(true);
    setPhase({ kind: "testing" });
    const started = await api.audio.start(side.channel, null);
    if (!started.ok) {
      running.current = false;
      onBusy(false);
      setPhase({ kind: "failed", message: started.error });
      return;
    }
    let peak = 0;
    const seen = (rms: number) => {
      peak = Math.max(peak, rms);
      setLevel(rms);
    };
    const stopListening = onEvent<{ rms: number }>(channelEventNames(side.channel).level, (payload) => seen(payload.rms));
    // A plain browser preview has no audio events, so a pretend signal shows the meter working.
    const fake = isTauriRuntime() ? undefined : window.setInterval(() => seen(Math.random() * 0.05), 120);
    let timer = 0;

    const release = async () => {
      window.clearTimeout(timer);
      window.clearInterval(fake);
      stopListening();
      stopNow.current = () => undefined;
      running.current = false;
      onBusy(false);
      await api.audio.stop(side.channel);
    };
    timer = window.setTimeout(() => {
      void release().then(() => {
        setLevel(0);
        setPhase(peak > HEARD || !isTauriRuntime() ? { kind: "heard", device: started.value.device_name } : { kind: "quiet" });
      });
    }, TEST_MS);
    stopNow.current = () => void release();
  }

  const testing = phase.kind === "testing";
  const meter = Math.min(1, Math.sqrt(level) * 3.4);
  const trouble = phase.kind === "failed" || phase.kind === "quiet";
  const text =
    phase.kind === "heard" ? "Heard sound"
    : phase.kind === "quiet" ? "Nothing heard"
    : phase.kind === "failed" ? phase.message
    : "";

  return (
    <li className="ac-row" data-phase={phase.kind}>
      <span className="ac-row__icon" aria-hidden="true">
        <Icon name={phase.kind === "heard" ? "check" : trouble ? "alert" : side.icon} size={15} />
      </span>
      <b>{side.name}</b>
      <span className="ac-meter" aria-hidden="true" data-on={testing || undefined}>
        <i style={{ transform: `scaleX(${testing ? Math.max(0.03, meter).toFixed(3) : 0})` }} />
      </span>
      <small className="ac-row__status" title={text} role={trouble ? "alert" : undefined}>{text}</small>
      {trouble ? <button type="button" className="ac-link" onClick={() => void openPrivacySettings(side.pane)}>Settings</button> : null}
      <button type="button" className="ac-test" disabled={disabled || testing} onClick={() => void test()}>
        {testing ? <span className="ac-spin" aria-hidden="true" /> : null}
        {testing ? "Listening" : phase.kind === "idle" ? "Test" : "Retest"}
      </button>
    </li>
  );
}

/**
 * Lets the person check that both sides work before the call (and brings up the
 * macOS permission questions now, not mid-meeting). Each test listens for a few
 * seconds and stops; nothing is recorded.
 * `onBusy` tells the page a test is holding the audio, so Start can wait.
 */
export function AudioCheck({ disabled, onBusy }: { disabled: boolean; onBusy: (busy: boolean) => void }) {
  const busy = useRef(0);
  const track = (on: boolean) => {
    busy.current = Math.max(0, busy.current + (on ? 1 : -1));
    onBusy(busy.current > 0);
  };
  return (
    <ul className="ac">
      {SIDES.map((side) => <Row key={side.channel} side={side} disabled={disabled} onBusy={track} />)}
    </ul>
  );
}
