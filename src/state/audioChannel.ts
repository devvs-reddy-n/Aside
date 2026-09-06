import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import { channelEventNames } from "../platform/eventNames";
import type {
  AudioDeviceInfo,
  CaptureInfo,
  CaptureStatus,
  ChannelId,
  ChannelLevelEvent,
  VadStatus,
} from "../platform/types";

/** Quiet speech is a small RMS number; this scale turns it into a 0-100
 *  meter percentage that actually moves visibly. Same scale the working
 *  diagnostic build used, carried over unchanged. */
const METER_SCALE = 400;

/** `null` means idle; otherwise which action is in flight, so a button can
 *  show "starting"/"stopping" instead of just being disabled. */
export type AudioBusyKind = "start" | "stop" | null;

export interface AudioChannelState {
  channel: ChannelId;
  /** False outside the real app — every consumer must show `error`
   *  instead of a picker/meter that can never do anything. */
  tauri: boolean;
  devices: AudioDeviceInfo[];
  /** The device pinned for this channel, or `null` to follow whatever the
   *  operating system currently calls the default. */
  deviceId: string | null;
  capturing: boolean;
  busy: AudioBusyKind;
  /** Live stream info (sample rate, device name) once capturing, `null`
   *  otherwise. */
  info: CaptureInfo | null;
  /** Most recent loudness reading, 0 (silent) to roughly 1 (very loud) —
   *  use `levelPercent` to turn this into a 0-100 meter width. */
  level: number;
  /** True while the voice-activity detector thinks someone is speaking on
   *  this ear right now. */
  speaking: boolean;
  /** A real problem (device open failed, permission denied, ...). */
  error: string | null;
  /** A non-fatal heads-up (e.g. "no audio heard yet on this device"), kept
   *  separate from `error` so it can render as guidance, not a red alert. */
  notice: string | null;
  start: () => Promise<boolean>;
  stop: () => Promise<void>;
  /** Stop, then start again on the currently chosen device — used after
   *  changing the speech engine, which needs a clean stream. */
  restart: () => Promise<void>;
  /** Saves the pin, and if this ear is already listening, restarts it on
   *  the new device so the switch takes effect immediately. */
  chooseDevice: (deviceId: string | null) => Promise<void>;
  refreshDevices: () => Promise<void>;
}

/** Turns a raw RMS reading into a 0-100 meter percentage. */
export function levelPercent(rms: number, scale = METER_SCALE): number {
  if (!Number.isFinite(rms) || rms <= 0) return 0;
  return Math.min(100, rms * scale);
}

function startCommand(channel: ChannelId) {
  return channel === "mic" ? "start_mic_capture" : "start_system_audio_capture";
}
function stopCommand(channel: ChannelId) {
  return channel === "mic" ? "stop_mic_capture" : "stop_system_audio_capture";
}
function listCommand(channel: ChannelId) {
  return channel === "mic" ? "list_input_devices" : "list_output_devices";
}
function getDeviceCommand(channel: ChannelId) {
  return channel === "mic" ? "get_mic_device" : "get_system_device";
}
function setDeviceCommand(channel: ChannelId) {
  return channel === "mic" ? "set_mic_device" : "set_system_device";
}
function getInfoCommand(channel: ChannelId) {
  return channel === "mic" ? "get_mic_capture_info" : "get_system_capture_info";
}
function capturingFlag(channel: ChannelId): keyof CaptureStatus {
  return channel === "mic" ? "mic_capturing" : "system_capturing";
}

/**
 * One audio ear (`"mic"` = you, `"system"` = everyone else on the call):
 * its device list, which device is chosen, whether it is currently
 * listening, its live level meter, and whether the voice-activity
 * detector currently thinks someone is speaking on it.
 *
 * Mirrors the working behaviour of the old diagnostic build's
 * `useAudioChannel`: starting is idempotent on the Rust side (a second
 * start while already capturing just returns the running stream's info),
 * stopping always clears the meter and info, and picking a new device
 * while already listening does a clean stop-then-start on the new device
 * rather than leaving two streams open.
 */
export function useAudioChannel(channel: ChannelId): AudioChannelState {
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const [capturing, setCapturing] = useState(false);
  const [busy, setBusy] = useState<AudioBusyKind>(null);
  const [info, setInfo] = useState<CaptureInfo | null>(null);
  const [level, setLevel] = useState(0);
  const [speaking, setSpeaking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const capturingRef = useRef(false);
  const deviceIdRef = useRef<string | null>(null);
  capturingRef.current = capturing;
  deviceIdRef.current = deviceId;

  const tauri = isTauriRuntime();
  const names = channelEventNames(channel);

  const refreshDevices = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const result = await invoke<AudioDeviceInfo[]>(listCommand(channel));
    if (result.ok) setDevices(result.value);
    else setError(result.error);
  }, [channel]);

  const startWith = useCallback(
    async (nextDevice: string | null): Promise<boolean> => {
      setBusy("start");
      setError(null);
      const result = await invoke<CaptureInfo>(startCommand(channel), { device: nextDevice });
      if (result.ok) {
        setInfo(result.value);
        setCapturing(true);
        setNotice(null);
        setBusy(null);
        return true;
      }
      setCapturing(false);
      setInfo(null);
      setSpeaking(false);
      setLevel(0);
      setError(result.error);
      setBusy(null);
      return false;
    },
    [channel],
  );

  const stop = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setBusy("stop");
    const result = await invoke<void>(stopCommand(channel));
    setCapturing(false);
    setInfo(null);
    setSpeaking(false);
    setLevel(0);
    if (!result.ok) setError(result.error);
    setBusy(null);
  }, [channel]);

  const start = useCallback(() => startWith(deviceIdRef.current), [startWith]);

  const restart = useCallback(async () => {
    await stop();
    await startWith(deviceIdRef.current);
  }, [startWith, stop]);

  const chooseDevice = useCallback(
    async (next: string | null) => {
      setDeviceId(next);
      const saved = await invoke<void>(setDeviceCommand(channel), { device: next });
      if (!saved.ok) setError(saved.error);
      if (capturingRef.current) {
        await stop();
        await startWith(next);
      }
    },
    [channel, startWith, stop],
  );

  // On mount: load the device list, the saved device pin, and whether this
  // channel is already capturing (e.g. this component remounted while a
  // stream from before kept running) — a query, not a redundant start.
  useEffect(() => {
    void refreshDevices();
    if (!isTauriRuntime()) return;
    let cancelled = false;
    (async () => {
      const pin = await invoke<string | null>(getDeviceCommand(channel));
      if (!cancelled && pin.ok) setDeviceId(pin.value);
      const status = await invoke<CaptureStatus>("get_capture_status");
      if (cancelled || !status.ok) return;
      if (status.value[capturingFlag(channel)]) {
        setCapturing(true);
        const live = await invoke<CaptureInfo | null>(getInfoCommand(channel));
        if (!cancelled && live.ok) setInfo(live.value);
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel]);

  useTauriEvent<ChannelLevelEvent>(names.level, (payload) => {
    setLevel(payload.rms);
    // The "no audio yet" notice is sticky until the meter actually moves —
    // once it does, the ear is proven live, so the stale warning should
    // not keep implying it looks dead.
    if (payload.rms > 0.002) setNotice(null);
  });
  useTauriEvent<string>(names.error, (payload) => setError(payload));
  useTauriEvent<string>(names.notice, (payload) => setNotice(payload));
  useTauriEvent<VadStatus>(names.vadState, (payload) => setSpeaking(payload.speaking));

  return {
    channel,
    tauri,
    devices,
    deviceId,
    capturing,
    busy,
    info,
    level,
    speaking,
    error,
    notice,
    start,
    stop,
    restart,
    chooseDevice,
    refreshDevices,
  };
}
