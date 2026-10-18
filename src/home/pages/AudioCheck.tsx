import * as api from "../../bridge/api";
import { onEvent } from "../../bridge/events";
import { channelEventNames } from "../../bridge/eventNames";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ChannelId } from "../../bridge/types";
import { runAudioProbe } from "../audioProbe";
export type { ProbeResult } from "../audioProbe";

/** Release the timer and abort listener on either completion or cancellation. */
function wait(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const done = () => { window.clearTimeout(timer); signal?.removeEventListener("abort", done); resolve(); };
    const timer = window.setTimeout(done, ms);
    signal?.addEventListener("abort", done, { once: true });
  });
}

/** Checks capture without transcription or saving, then releases both devices. */
export function probeChannel(channel: ChannelId, options: { ms?: number; onLevel?: (rms: number) => void; signal?: AbortSignal } = {}) {
  return runAudioProbe(channel, {
    info: () => api.audio.info(channel),
    pinned: () => api.audio.pinnedDevice(channel),
    start: (device) => api.audio.start(channel, device),
    stop: () => api.audio.stop(channel),
    playTone: (device) => api.audio.playTestSound(device),
    wait,
    listen: (level, error) => {
      const names = channelEventNames(channel);
      const stopLevel = onEvent<{ rms: number }>(names.level, (payload) => level(payload.rms));
      const stopError = onEvent<string>(names.error, error);
      // Development preview only: native checks always use measured device audio.
      const fake = isTauriRuntime() ? undefined : window.setInterval(() => level(0.025), 90);
      return () => { window.clearInterval(fake); stopLevel(); stopError(); };
    },
  }, options);
}
