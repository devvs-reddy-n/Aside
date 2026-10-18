import type { CaptureInfo, ChannelId } from "../bridge/types";
import type { Result } from "../bridge/api";

export type ProbeResult =
  | { kind: "heard"; device: string | null }
  | { kind: "quiet"; device: string | null; soundError?: string }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

export interface ProbeIO {
  info(): Promise<Result<CaptureInfo | null>>;
  pinned(): Promise<Result<string | null>>;
  start(device: string | null): Promise<Result<CaptureInfo>>;
  stop(): Promise<unknown>;
  listen(level: (rms: number) => void, error: (message: string) => void): () => void;
  playTone(device: string | null): Promise<Result<void>>;
  wait(ms: number, signal?: AbortSignal): Promise<void>;
}

/** Two measured buffers prove capture. Stop waiting as soon as evidence arrives. */
export async function runAudioProbe(channel: ChannelId, io: ProbeIO, options: { signal?: AbortSignal; ms?: number; onLevel?: (rms: number) => void } = {}): Promise<ProbeResult> {
  const { signal } = options;
  if (signal?.aborted) return { kind: "cancelled" };
  const [existing, pinned] = await Promise.all([io.info(), io.pinned()]);
  if (!existing.ok) return { kind: "failed", message: existing.error };
  if (existing.value) return { kind: "failed", message: "Audio is already in use. End the current call before checking again." };
  if (!pinned.ok) return { kind: "failed", message: pinned.error };
  if (signal?.aborted) return { kind: "cancelled" };
  let samples = 0;
  let heard = false;
  let failure: string | null = null;
  let started = false;
  const observation = new AbortController();
  const warmed = new AbortController();
  const cancel = () => { observation.abort(); warmed.abort(); };
  signal?.addEventListener("abort", cancel, { once: true });
  const unlisten = io.listen((rms) => {
    if (!Number.isFinite(rms) || rms < 0) return;
    warmed.abort(); // A real buffer is a better readiness signal than a fixed delay.
    samples = rms > 0.002 ? samples + 1 : 0;
    if (samples >= 2) { heard = true; observation.abort(); }
    options.onLevel?.(rms);
  }, (message) => { failure = message; cancel(); });
  let result: ProbeResult;
  try {
    const capture = await io.start(pinned.value);
    if (!capture.ok) return { kind: "failed", message: capture.error };
    started = true;
    if (signal?.aborted) return { kind: "cancelled" };
    let soundError: string | undefined;
    if (channel === "system" && !heard && !failure) {
      await io.wait(200, warmed.signal);
      if (signal?.aborted) return { kind: "cancelled" };
      if (!heard && !failure) {
        const sound = await io.playTone(capture.value.device_id);
        if (!sound.ok) soundError = sound.error;
      }
    }
    if (!heard && !failure) await io.wait(options.ms ?? (channel === "mic" ? 2200 : 1200), observation.signal);
    result = signal?.aborted ? { kind: "cancelled" }
      : failure ? { kind: "failed", message: failure }
      : heard ? { kind: "heard", device: capture.value.device_name }
      : { kind: "quiet", device: capture.value.device_name, soundError };
  } catch {
    result = signal?.aborted ? { kind: "cancelled" } : { kind: "failed", message: "Couldn't finish the audio check. Try again." };
  } finally {
    cancel();
    signal?.removeEventListener("abort", cancel);
    try {
      if (started) {
        const stopped = await io.stop();
        if (stopped && typeof stopped === "object" && "ok" in stopped && !stopped.ok) {
          result = { kind: "failed", message: "Couldn't release the audio device. Close this panel and try again." };
        }
      }
    } catch {
      result = { kind: "failed", message: "Couldn't release the audio device. Close this panel and try again." };
    } finally {
      unlisten();
      options.onLevel?.(0);
    }
  }
  return result!;
}
