import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS } from "../src/diagnostics/layers.ts";
import {
  applyAnswerDelta,
  applyAnswerDone,
  applyAnswerError,
  applyAnswerStarted,
  applyInvokeError,
  applyQuestionCleaned,
  CUSTOM_MODEL,
  displayQuestion,
  FALLBACK_MODELS,
  idleLlmInspector,
  modelLabel,
  providerLabel,
  selectValueForSaved,
  trimmedQuestion,
} from "../src/diagnostics/llmModel.ts";

const here = dirname(fileURLToPath(import.meta.url));
const inspector = readFileSync(join(here, "../src/diagnostics/LlmInspector.tsx"), "utf8");
const model = readFileSync(join(here, "../src/diagnostics/llmModel.ts"), "utf8");
const app = readFileSync(join(here, "../src/diagnostics/DiagnosticApp.tsx"), "utf8");
const shortcuts = readFileSync(join(here, "../src-tauri/src/shortcuts.rs"), "utf8");
const turnTable = readFileSync(join(here, "../src/diagnostics/TurnTable.tsx"), "utf8");
const sttInspector = readFileSync(join(here, "../src/diagnostics/SttInspector.tsx"), "utf8");
const pkg = readFileSync(join(here, "../package.json"), "utf8");

{
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 4)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 4)?.page, "Ask");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 4)?.name, "Type a question");
  assert.ok(
    DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"),
    "layers 9–11 stay locked",
  );
}

{
  assert.match(app, /page === 4 \? <LlmInspector/);
  assert.match(inspector, /trigger_answer/);
  const sessionIds = [...inspector.matchAll(/sessionId:\s*(\S+)/g)].map((match) =>
    match[1].replace(/,$/, ""),
  );
  assert.deepEqual(sessionIds, ["null"], "Ask must pass sessionId: null and nothing else");
}

{
  assert.match(inspector, /listen<AnswerStartedPayload>\(LLM_EVENTS\.started/);
  assert.match(inspector, /listen<AnswerDeltaPayload>\(LLM_EVENTS\.delta/);
  assert.match(inspector, /listen<AnswerDonePayload>\(LLM_EVENTS\.done/);
  assert.match(inspector, /listen<AnswerErrorPayload>\(LLM_EVENTS\.error/);
  assert.match(inspector, /listen<QuestionCleanedPayload>\(LLM_EVENTS\.cleaned/);
  for (const event of [
    "answer-started",
    "answer-delta",
    "answer-done",
    "answer-error",
    "question-cleaned",
  ]) {
    assert.match(model, new RegExp(event.replaceAll("-", "\\-")));
  }
}

{
  const banned = [
    "get_last_you_turn",
    "capture_screen_and_analyze",
    "start_session_capture",
    "looks_like_a_question",
    "create_session",
    "commit_briefing",
    "show_panel",
    "show_bench",
    "ListenBar",
    "useAudioChannel",
    "get_llm_api_key",
    "anthropic",
    "gemini",
    "groq",
    "framer-motion",
    "phosphor",
  ];
  for (const token of banned) {
    assert.doesNotMatch(inspector, new RegExp(token, "i"), `Ask must not mention ${token}`);
    assert.doesNotMatch(model, new RegExp(token, "i"), `llmModel must not mention ${token}`);
  }
}

{
  assert.match(inspector, /type="password"/);
  assert.match(inspector, /has_llm_api_key/);
  assert.match(inspector, /set_llm_api_key/);
  assert.match(inspector, /list_llm_providers/);
  assert.match(inspector, /list_llm_models/);
  assert.match(inspector, /get_llm_model/);
  assert.match(inspector, /set_llm_model/);
  assert.match(inspector, /selectedModel/);
  assert.match(inspector, /SttEnginePicker/);
  assert.match(inspector, /useSttEngine/);
  assert.doesNotMatch(inspector, /restartLiveChannels/);
  assert.doesNotMatch(inspector, /start_channel_transcription/);
  assert.doesNotMatch(inspector, /value=\{provider \?\? ""\}/);
  assert.doesNotMatch(inspector, /autoAnswer:\s*true/);
}

{
  assert.match(shortcuts, /fn diagnostic_cmd_enter_may_call_llm\(\) -> bool/);
  assert.match(
    shortcuts,
    /pub fn diagnostic_screen_hotkey_may_call_llm\(\) -> bool \{\n    false\n\}/,
  );
}

{
  for (const banned of ["trigger_answer", "start_session_capture", "looks_like_a_question", "answer-delta"]) {
    assert.doesNotMatch(turnTable, new RegExp(banned));
    assert.doesNotMatch(sttInspector, new RegExp(banned));
  }
}

{
  assert.doesNotMatch(pkg, /framer-motion/);
  assert.doesNotMatch(pkg, /phosphor/);
}

{
  assert.equal(providerLabel("openai"), "OpenAI");
  assert.equal(modelLabel("gpt-5.6-luna"), "GPT-5.6 Luna");
  assert.equal(modelLabel("gpt-5.6-terra"), "GPT-5.6 Terra");
  assert.equal(modelLabel("gpt-5.6-sol"), "GPT-5.6 Sol");
  assert.equal(FALLBACK_MODELS[0], "gpt-5.6-luna");
  assert.ok(FALLBACK_MODELS.includes("gpt-5.6-terra"));
  assert.equal(selectValueForSaved(null, [...FALLBACK_MODELS]), "gpt-5.6-luna");
  assert.equal(selectValueForSaved("gpt-5.6-terra", [...FALLBACK_MODELS]), "gpt-5.6-terra");
  assert.equal(selectValueForSaved("gpt-4o-mini", [...FALLBACK_MODELS]), CUSTOM_MODEL);
  assert.equal(trimmedQuestion("   "), null);
  assert.equal(trimmedQuestion("What is Kubernetes?"), "What is Kubernetes?");

  let state = idleLlmInspector();
  assert.equal(state.question, "");
  assert.equal(displayQuestion(state), "");

  state = applyAnswerStarted(state, { question: "What is Kubernetes?" });
  assert.equal(state.question, "What is Kubernetes?");
  assert.equal(state.text, "", "started must clear previous answer text");
  assert.equal(state.streaming, true);
  assert.equal(state.canned, false);

  state = applyAnswerDelta(state, { question: "What is Kubernetes?", text: "A " });
  state = applyAnswerDelta(state, { question: "What is Kubernetes?", text: "cluster." });
  assert.equal(state.text, "A cluster.", "same-question deltas append");

  state = applyQuestionCleaned(state, {
    question: "What is Kubernetes?",
    cleaned_question: "",
  });
  assert.equal(displayQuestion(state), "What is Kubernetes?", "empty cleaned falls back to typed");

  state = applyQuestionCleaned(state, {
    question: "What is Kubernetes?",
    cleaned_question: "What is Kubernetes?",
  });
  assert.equal(displayQuestion(state), "What is Kubernetes?");

  state = applyAnswerDone(state, { question: "What is Kubernetes?", canned: true });
  assert.equal(state.canned, true);
  assert.equal(state.streaming, false);

  state = applyAnswerStarted(state, { question: "Second question" });
  state = applyAnswerDelta(state, { question: "Second question", text: "New." });
  assert.equal(state.text, "New.", "a new question replaces the previous stream");
  assert.equal(state.canned, false);

  state = applyAnswerError(state, { question: "Second question", message: "network" });
  assert.equal(state.error, "network");
  assert.equal(state.streaming, false);

  const afterError = applyAnswerError(state, { question: "Someone else", message: "stale" });
  assert.equal(afterError.error, "network", "a stale error must not overwrite the current question");

  const invoked = applyInvokeError(idleLlmInspector(), "question cannot be empty");
  assert.equal(invoked.error, "question cannot be empty");
  assert.equal(invoked.streaming, false);
}

console.log("verify-llm-inspector: ok");
