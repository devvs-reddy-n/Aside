import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS } from "../src/diagnostics/layers.ts";
import {
  ENGINE_ORDER,
  applyTranscript,
  engineLabel,
  engineNeedsKey,
  formatLatency,
  idleSttInspector,
  sideTitle,
  sttConnection,
} from "../src/diagnostics/sttModel.ts";

const here = dirname(fileURLToPath(import.meta.url));
const inspector = readFileSync(join(here, "../src/diagnostics/SttInspector.tsx"), "utf8");
const model = readFileSync(join(here, "../src/diagnostics/sttModel.ts"), "utf8");
const hook = readFileSync(join(here, "../src/diagnostics/useSttEngine.ts"), "utf8");
const picker = readFileSync(join(here, "../src/diagnostics/SttEnginePicker.tsx"), "utf8");
const diagnosticsDir = join(here, "../src/diagnostics");

{
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 1)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 2)?.status, "ready");
  assert.ok(
    DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"),
    "layers 9–11 stay locked",
  );
}

{
  assert.deepEqual(ENGINE_ORDER, ["speechmatics", "deepgram", "parakeet_local"]);
  assert.equal(engineLabel("speechmatics"), "Speechmatics");
  assert.equal(engineLabel("deepgram"), "Deepgram");
  assert.match(engineLabel("parakeet_local"), /Parakeet Realtime EOU 120M/);
  assert.equal(engineNeedsKey("parakeet_local"), false);
  assert.equal(engineNeedsKey("deepgram"), true);
  assert.equal(sideTitle("mic"), "Me");
  assert.equal(sideTitle("system"), "Others");
}

{
  let id = 0;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = idleSttInspector();
  state = applyTranscript(
    state,
    "mic",
    { text: "My current work authorization is STEM OPT", is_final: false },
    "speechmatics",
    1000,
    nextId,
  );
  assert.equal(state.mic.interim, "My current work authorization is STEM OPT");
  assert.equal(state.mic.finals.length, 0);
  assert.equal(state.system.interim, "", "system column must stay empty");
  assert.equal(state.system.finals.length, 0);

  state = applyTranscript(
    state,
    "mic",
    { text: "My current work authorization is STEM OPT and I work on AWS and Kubernetes.", is_final: true },
    "speechmatics",
    1800,
    nextId,
  );
  assert.equal(state.mic.interim, "", "final must clear the interim line");
  assert.equal(state.mic.finals.length, 1);
  assert.match(state.mic.finals[0].text, /STEM OPT/);
  assert.equal(state.mic.lastLatencyMs, 800);
  assert.equal(state.system.finals.length, 0, "mic final must not appear under OTHER PARTY");

  state = applyTranscript(
    state,
    "system",
    { text: "Can you explain your Kubernetes experience?", is_final: true },
    "speechmatics",
    2400,
    nextId,
  );
  assert.equal(state.system.finals.length, 1);
  assert.match(state.system.finals[0].text, /Kubernetes experience/);
  assert.equal(state.mic.finals.length, 1, "system final must not append onto MY SPEECH");
  assert.ok(!state.mic.finals[0].text.includes("Kubernetes experience"));
}

assert.equal(
  sttConnection({ capturing: true, transcribing: true, gotFinal: false, error: null, busy: false }),
  "transcribing",
);
assert.equal(
  sttConnection({ capturing: true, transcribing: true, gotFinal: true, error: null, busy: false }),
  "live",
);
assert.equal(formatLatency(null), "—");
assert.equal(formatLatency(800), "800 ms");

for (const banned of ["trigger_answer", "start_session_capture", "looks_like_a_question", "answer-delta"]) {
  assert.ok(!inspector.includes(banned), `STT Inspector must not call ${banned}`);
  assert.ok(!model.includes(banned), `sttModel must not mention ${banned}`);
}

assert.match(inspector, /autoAnswer:\s*false/);
assert.match(inspector, /sessionId:\s*null/, "Layer 2 must stay live-only (no notebook writes)");
assert.match(inspector, /start_channel_transcription/);
assert.match(inspector, /STT_EVENTS\.micTranscript/);
assert.match(inspector, /STT_EVENTS\.systemTranscript/);
assert.match(model, /mic-transcript/);
assert.match(model, /system-transcript/);
assert.match(inspector, /useAudioChannel\("mic"\)/);
assert.match(inspector, /useAudioChannel\("system"\)/);
assert.match(inspector, /Me/);
assert.match(inspector, /Others/);
assert.match(inspector, /listenBoth/);
assert.match(inspector, /Save a \$\{engineLabel\(stt\.engine\)\} API key first/);
assert.match(inspector, /restartLiveChannels/);
assert.match(inspector, /SttEnginePicker/);
assert.match(inspector, /useSttEngine/);

{
  assert.match(hook, /list_diagnostic_stt_engines/);
  assert.match(hook, /set_diagnostic_stt_engine/);
  assert.match(hook, /get_stt_engine/);
  assert.match(hook, /restartRef\.current/);
  assert.match(hook, /stopRef\.current/);
  assert.doesNotMatch(hook, /autoAnswer:\s*true/);
  assert.match(picker, /aria-label="Speech engine"/);
  assert.match(picker, /stt\.chooseEngine/);
  assert.match(picker, /engineLabel\(kind\)/);
}

{
  const readyPages = [
    "AudioInspector.tsx",
    "SttInspector.tsx",
    "TurnTable.tsx",
    "LlmInspector.tsx",
    "MicSubmitInspector.tsx",
    "AssistTaskInspector.tsx",
  ];
  for (const name of readyPages) {
    const src = readFileSync(join(diagnosticsDir, name), "utf8");
    assert.match(src, /SttEnginePicker/, `${name} must show the speech-engine dropdown`);
    assert.match(src, /useSttEngine/, `${name} must save through the shared speech-engine hook`);
  }
}

{
  const listenPages = ["SttInspector.tsx", "TurnTable.tsx", "MicSubmitInspector.tsx", "AssistTaskInspector.tsx"];
  for (const name of listenPages) {
    const src = readFileSync(join(diagnosticsDir, name), "utf8");
    assert.match(src, /restartLiveChannels/, `${name} must restart live ears after an engine change`);
    assert.match(src, /start_channel_transcription/);
  }
  const persistOnly = ["AudioInspector.tsx", "LlmInspector.tsx"];
  for (const name of persistOnly) {
    const src = readFileSync(join(diagnosticsDir, name), "utf8");
    assert.doesNotMatch(
      src,
      /start_channel_transcription/,
      `${name} must only save the engine, not start transcription`,
    );
    assert.doesNotMatch(src, /restartLiveChannels/);
  }
}

console.log("verify-stt-inspector: ok");
