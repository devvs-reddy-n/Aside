import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS, INITIAL_PAGE } from "../src/diagnostics/layers.ts";
import { EMPTY_YOU_NOTICE } from "../src/diagnostics/MicSubmitInspector.tsx";
import { trimmedQuestion } from "../src/diagnostics/llmModel.ts";

const here = dirname(fileURLToPath(import.meta.url));
const inspector = readFileSync(join(here, "../src/diagnostics/MicSubmitInspector.tsx"), "utf8");
const app = readFileSync(join(here, "../src/diagnostics/DiagnosticApp.tsx"), "utf8");
const shortcuts = readFileSync(join(here, "../src-tauri/src/shortcuts.rs"), "utf8");
const ask = readFileSync(join(here, "../src/diagnostics/LlmInspector.tsx"), "utf8");
const turnTable = readFileSync(join(here, "../src/diagnostics/TurnTable.tsx"), "utf8");
const sttInspector = readFileSync(join(here, "../src/diagnostics/SttInspector.tsx"), "utf8");

{
  assert.equal(INITIAL_PAGE, 6);
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 5)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 5)?.page, "Mic submit");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 4)?.status, "ready");
  assert.ok(
    DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"),
    "layers 9–11 stay locked",
  );
}

{
  assert.match(app, /page === 5 \? <MicSubmitInspector/);
  assert.match(inspector, /trigger_last_you_answer/);
  assert.doesNotMatch(inspector, /"trigger_answer"/);
  assert.match(inspector, /start_channel_transcription/);
  assert.match(inspector, /channel:\s*"mic"/);
  assert.match(inspector, /sessionId:\s*null/);
  assert.match(inspector, /autoAnswer:\s*false/);
  assert.match(inspector, /metaKey \|\| event.ctrlKey/);
  assert.match(inspector, /shortcut-notice/);
  assert.match(inspector, /meOnly/);
  assert.match(inspector, /SttEnginePicker/);
  assert.match(inspector, /useSttEngine/);
  assert.match(inspector, /restartLiveChannels/);
  assert.doesNotMatch(inspector, /start_system_audio_capture/);
  assert.doesNotMatch(inspector, /create_session/);
  assert.doesNotMatch(inspector, /capture_screen_and_analyze/);
  assert.doesNotMatch(inspector, /start_session_capture/);
  assert.doesNotMatch(inspector, /autoAnswer:\s*true/);
}

{
  assert.match(
    shortcuts,
    /pub fn diagnostic_cmd_enter_may_call_llm\(\) -> bool \{\n    true\n\}/,
  );
  assert.match(
    shortcuts,
    /pub fn diagnostic_screen_hotkey_may_call_llm\(\) -> bool \{\n    false\n\}/,
  );
  assert.match(shortcuts, /respond_to_window/);
  assert.doesNotMatch(
    shortcuts,
    /if !diagnostic_hotkeys_may_call_llm\(\)/,
    "shared hotkey gate must not block both shortcuts",
  );
}

{
  assert.equal(
    EMPTY_YOU_NOTICE,
    "Say something first — Aside answers the last thing you said.",
  );
  assert.equal(trimmedQuestion(""), null);
  assert.equal(trimmedQuestion("   "), null);
  assert.equal(trimmedQuestion("What is Kubernetes?"), "What is Kubernetes?");
}

{
  assert.doesNotMatch(ask, /get_last_you_turn/);
  assert.doesNotMatch(ask, /start_channel_transcription/);
  for (const banned of ["trigger_answer", "get_last_you_turn", "answer-delta"]) {
    assert.doesNotMatch(turnTable, new RegExp(banned));
    assert.doesNotMatch(sttInspector, new RegExp(banned));
  }
}

console.log("verify-mic-submit: ok");
