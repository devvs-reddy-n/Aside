import { useEffect, useRef, useState } from "react";
import * as api from "../../bridge/api";
import { onEvent } from "../../bridge/events";
import { channelEventNames } from "../../bridge/eventNames";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ChannelId } from "../../bridge/types";

/** Loudness above which a side is proven to be hearing something (same bar the live window uses). */
const HEARD = 0.002;

export type ProbeResult =
  | { kind: "heard"; device: string | null }
  | { kind: "quiet"; device: string | null }
  | { kind: "failed"; message: string };

/**
 * Listens on one side for a few seconds, then lets go. Nothing is recorded; it only
 * proves the side works (and brings up the macOS permission question now, not mid-call).
 * `onLevel` gets the live loudness for a meter. Returns early if `signal` is aborted.
 */
export async function probeChannel(channel: ChannelId, options: { ms?: number; onLevel?: (rms: number) => void; signal?: AbortSignal } = {}): Promise<ProbeResult> {
  const started = await api.audio.start(channel, null);
  if (!started.ok) return { kind: "failed", message: started.error };
  let peak = 0;
  const seen = (rms: number) => {
    peak = Math.max(peak, rms);
    options.onLevel?.(rms);
  };
  const stopListening = onEvent<{ rms: number }>(channelEventNames(channel).level, (payload) => seen(payload.rms));
  // A plain browser preview has no audio events, so a pretend signal shows the meter working.
  const fake = isTauriRuntime() ? undefined : window.setInterval(() => seen(Math.random() * 0.05), 90);
  await new Promise<void>((resolve) => {
    const timer = window.setTimeout(resolve, options.ms ?? 2500);
    options.signal?.addEventListener("abort", () => {
      window.clearTimeout(timer);
      resolve();
    });
  });
  window.clearInterval(fake);
  stopListening();
  options.onLevel?.(0);
  await api.audio.stop(channel);
  const heard = peak > HEARD || !isTauriRuntime();
  return heard ? { kind: "heard", device: started.value.device_name } : { kind: "quiet", device: started.value.device_name };
}

/**
 * Five bars that follow loudness: they jump up fast (50 ms) and fall back slowly (200 ms),
 * like a studio meter, so speech reads as steady rather than flickering.
 */
export function LevelBars({ level, active }: { level: number; active: boolean }) {
  const target = useRef(0);
  const [shown, setShown] = useState(0);
  target.current = active ? Math.min(1, Math.sqrt(level) * 3.4) : 0;

  useEffect(() => {
    let frame = 0;
    let last = performance.now();
    let value = 0;
    const tick = (now: number) => {
      const dt = now - last;
      last = now;
      const goal = target.current;
      const tau = goal > value ? 50 : 200;
      value += (goal - value) * (1 - Math.exp(-dt / tau));
      setShown((current) => (Math.abs(current - value) > 0.01 ? value : current));
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  const lit = Math.round(shown * 5);
  return (
    <span className="bars" data-active={active || undefined} aria-hidden="true">
      {[0, 1, 2, 3, 4].map((index) => <i key={index} data-on={index < lit || undefined} />)}
    </span>
  );
}
