import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  applyChannelEvent,
  channelEvents,
  channelTitle,
  connectionLabel,
  connectionStatus,
  formatElapsed,
  idleInspector,
  levelPercent,
  speechDetected,
  startCommand,
  stopCommand,
} from "../src/diagnostics/audioModel.ts";
import { DIAGNOSTIC_LAYERS } from "../src/diagnostics/layers.ts";

const here = dirname(fileURLToPath(import.meta.url));
const diagnosticsDir = join(here, "../src/diagnostics");
const layer1Files = ["AudioInspector.tsx", "ChannelInspector.tsx", "audioModel.ts", "useAudioChannel.ts"];

function readLayer1Sources(): string {
  return layer1Files.map((name) => readFileSync(join(diagnosticsDir, name), "utf8")).join("\n");
}

const sources = readLayer1Sources();

{
  const audio = DIAGNOSTIC_LAYERS.find((layer) => layer.id === 1);
  assert.equal(audio?.status, "ready");
  assert.equal(audio?.page, "Audio");
}

// --- Independent channel state ---

{
  let state = idleInspector();
  state = applyChannelEvent(state, "mic", { capturing: true, rms: 0.2, speaking: true });
  assert.equal(state.mic.capturing, true);
  assert.equal(state.mic.rms, 0.2);
  assert.equal(state.system.capturing, false);
  assert.equal(state.system.rms, 0);
  assert.equal(state.system.speaking, false);

  state = applyChannelEvent(state, "system", { capturing: true, rms: 0.05 });
  assert.equal(state.mic.rms, 0.2, "system start must not overwrite mic level");
  assert.equal(state.system.capturing, true);

  state = applyChannelEvent(state, "mic", { capturing: false, rms: 0, speaking: false });
  assert.equal(state.system.capturing, true, "stopping the mic must leave system audio running");
}

{
  const mic = channelEvents("mic");
  const system = channelEvents("system");
  assert.equal(mic.level, "mic-level");
  assert.equal(system.level, "system-level");
  assert.notEqual(mic.level, system.level);
  assert.notEqual(mic.vadState, system.vadState);
  assert.equal(startCommand("mic"), "start_mic_capture");
  assert.equal(startCommand("system"), "start_system_audio_capture");
  assert.equal(stopCommand("mic"), "stop_mic_capture");
  assert.equal(stopCommand("system"), "stop_system_audio_capture");
  assert.equal(channelTitle("mic"), "Me");
  assert.equal(channelTitle("system"), "Others");
}

// --- Meter / elapsed / speech math ---

assert.equal(levelPercent(0), 0);
assert.equal(levelPercent(-1), 0);
assert.equal(levelPercent(0.1), 40);
assert.equal(levelPercent(1), 100);
assert.equal(formatElapsed(0), "00:00");
assert.equal(formatElapsed(1000), "00:01");
assert.equal(formatElapsed(61_000), "01:01");
assert.equal(formatElapsed(3_600_000), "1:00:00");
assert.equal(speechDetected(false, true), false, "idle channel is never 'speaking'");
assert.equal(speechDetected(true, true), true);
assert.equal(speechDetected(true, false), false);
assert.equal(speechDetected(true, null), false);
assert.equal(connectionLabel(connectionStatus({ capturing: true, busy: null, error: null })), "Listening");
assert.equal(
  connectionLabel(connectionStatus({ capturing: false, busy: "start", error: null })),
  "Starting",
);
assert.equal(
  connectionLabel(connectionStatus({ capturing: false, busy: null, error: "denied" })),
  "Error",
);

// --- Layer 1 must not wire STT or LLM ---

for (const banned of [
  "start_channel_transcription",
  "stop_channel_transcription",
  "start_session_capture",
  "trigger_answer",
  "looks_like_a_question",
  "answer-delta",
  "mic-transcript",
  "system-transcript",
]) {
  assert.ok(!sources.includes(banned), `Layer 1 diagnostics must not mention ${banned}`);
}

assert.match(sources, /start_mic_capture/);
assert.match(sources, /start_system_audio_capture/);
assert.match(sources, /\$\{channel\}-vad-state/);
assert.match(sources, /\$\{channel\}-level/);
assert.match(sources, /get_mic_capture_info/);
assert.match(sources, /get_system_capture_info/);
assert.match(sources, /names\.vadState/);

const inspector = readFileSync(join(diagnosticsDir, "AudioInspector.tsx"), "utf8");
assert.match(inspector, /useAudioChannel\("mic"\)/);
assert.match(inspector, /useAudioChannel\("system"\)/);
assert.doesNotMatch(inspector, /useAudioChannel\("both"\)/);
assert.match(inspector, /SttEnginePicker/);
assert.match(inspector, /useSttEngine/);
assert.doesNotMatch(inspector, /restartLiveChannels/);
assert.doesNotMatch(inspector, /start_channel_transcription/);

console.log("verify-audio-inspector: ok");
