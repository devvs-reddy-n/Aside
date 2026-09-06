import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS, INITIAL_PAGE } from "../src/diagnostics/layers.ts";
import { EMPTY_ASSIST_NOTICE } from "../src/diagnostics/AssistTaskInspector.tsx";
import { EMPTY_YOU_NOTICE } from "../src/diagnostics/MicSubmitInspector.tsx";
import { describeShortcut } from "../src/lib/shortcutCopy.ts";

const here = dirname(fileURLToPath(import.meta.url));
const inspector = readFileSync(join(here, "../src/diagnostics/AssistTaskInspector.tsx"), "utf8");
const app = readFileSync(join(here, "../src/diagnostics/DiagnosticApp.tsx"), "utf8");
const shortcuts = readFileSync(join(here, "../src-tauri/src/shortcuts.rs"), "utf8");
const conversation = readFileSync(join(here, "../src-tauri/src/conversation/mod.rs"), "utf8");
const assistMod = readFileSync(join(here, "../src-tauri/src/assist/mod.rs"), "utf8");
const classify = readFileSync(join(here, "../src-tauri/src/assist/classify.rs"), "utf8");
const pick = readFileSync(join(here, "../src-tauri/src/assist/pick.rs"), "utf8");
const fixtures = readFileSync(join(here, "../src-tauri/src/assist/fixtures.rs"), "utf8");
const mic = readFileSync(join(here, "../src/diagnostics/MicSubmitInspector.tsx"), "utf8");

{
  assert.equal(INITIAL_PAGE, 6);
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 6)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 7)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 8)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 5)?.status, "ready");
  assert.ok(
    DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"),
    "layers 9–11 stay locked",
  );
}

{
  assert.match(app, /page === 6 \? <AssistTaskInspector focus="facts"/);
  assert.match(app, /page === 7 \? <AssistTaskInspector focus="context"/);
  assert.match(app, /page === 8 \? <AssistTaskInspector focus="memory"/);
  assert.match(app, /page === 5 \? <MicSubmitInspector/);
}

{
  assert.match(inspector, /create_session/);
  assert.match(inspector, /autoAnswer:\s*false/);
  assert.match(inspector, /start_channel_transcription/);
  assert.match(inspector, /channel:\s*"mic"/);
  assert.match(inspector, /channel:\s*"system"/);
  assert.match(inspector, /list_session_facts/);
  assert.match(inspector, /get_window_preview/);
  assert.match(inspector, /trigger_window_answer/);
  assert.match(inspector, /data-testid="fact-ledger"/);
  assert.match(inspector, /data-testid="open-task"/);
  assert.match(inspector, /data-testid="context-debug"/);
  assert.match(inspector, /SttEnginePicker/);
  assert.match(inspector, /useSttEngine/);
  assert.match(inspector, /restartLiveChannels/);
  assert.doesNotMatch(inspector, /autoAnswer:\s*true/);
  assert.doesNotMatch(inspector, /capture_screen_and_analyze/);
  assert.doesNotMatch(inspector, /start_session_capture/);
}

{
  assert.match(shortcuts, /respond_to_window/);
  assert.match(shortcuts, /Ask about the live chat/);
  assert.match(
    shortcuts,
    /pub fn diagnostic_cmd_enter_may_call_llm\(\) -> bool \{\n    true\n\}/,
  );
  assert.match(
    shortcuts,
    /pub fn diagnostic_screen_hotkey_may_call_llm\(\) -> bool \{\n    false\n\}/,
  );
  assert.doesNotMatch(shortcuts, /auto_answer:\s*true/);
}

{
  assert.equal(
    EMPTY_ASSIST_NOTICE,
    "Nothing to send yet — talk first, then press Command+Enter.",
  );
  assert.match(pick, /pub const EMPTY_NOTICE/);
  assert.equal(describeShortcut("CmdOrCtrl+Enter"), "Ask about the live chat");
}

{
  assert.match(assistMod, /pub mod classify/);
  assert.match(assistMod, /pub mod assemble/);
  assert.match(assistMod, /pub mod pick/);
  assert.match(classify, /looks_like_a_question/);
  assert.doesNotMatch(classify, /respond_to_question/);
  assert.match(fixtures, /user_walkthrough_becomes_the_prompt/);
  assert.match(fixtures, /spoken_stem_opt_overrides_document_h1b/);
  assert.match(fixtures, /multi_turn_other_with_user_okay_is_one_task/);
  assert.match(fixtures, /follow_up_fts_finds_the_earlier_migration/);
  assert.match(fixtures, /ack_after_send_does_not_restart/);
}

{
  assert.match(conversation, /if !auto_answer/);
  assert.match(conversation, /ack_after_send_does_not_restart_or_cancel/);
}

{
  assert.equal(
    EMPTY_YOU_NOTICE,
    "Say something first — Aside answers the last thing you said.",
  );
  assert.match(mic, /sessionId:\s*null/);
  assert.match(mic, /trigger_last_you_answer/);
}

console.log("ok");
