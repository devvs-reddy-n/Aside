import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS, INITIAL_PAGE } from "../src/diagnostics/layers.ts";
import { EMPTY_ASSIST_NOTICE } from "../src/diagnostics/AssistTaskInspector.tsx";
import { EMPTY_YOU_NOTICE } from "../src/diagnostics/MicSubmitInspector.tsx";

const here = dirname(fileURLToPath(import.meta.url));
const shortcuts = readFileSync(join(here, "../src-tauri/src/shortcuts.rs"), "utf8");
const inspector = readFileSync(join(here, "../src/diagnostics/AssistTaskInspector.tsx"), "utf8");
const mic = readFileSync(join(here, "../src/diagnostics/MicSubmitInspector.tsx"), "utf8");
const llm = readFileSync(join(here, "../src-tauri/src/llm/mod.rs"), "utf8");
const pipeline = readFileSync(join(here, "../src-tauri/src/pipeline.rs"), "utf8");
const windowMod = readFileSync(join(here, "../src-tauri/src/window_memory/mod.rs"), "utf8");
const parse = readFileSync(join(here, "../src-tauri/src/window_memory/parse.rs"), "utf8");
const merge = readFileSync(join(here, "../src-tauri/src/window_memory/merge.rs"), "utf8");
const prompt = readFileSync(join(here, "../src-tauri/src/window_memory/prompt.rs"), "utf8");

assert.equal(INITIAL_PAGE, 6);
assert.ok(DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"));

assert.match(shortcuts, /respond_to_window/);
assert.match(llm, /window_memory::freeze/);
assert.match(llm, /try_begin_user_answer/);
assert.match(llm, /EMPTY_WINDOW_NOTICE/);
assert.doesNotMatch(shortcuts, /assist::pick_for_app/);
assert.match(shortcuts, /diagnostic_cmd_enter_may_call_llm\(\) -> bool \{\n    true\n\}/);
assert.match(
  shortcuts,
  /pub fn diagnostic_screen_hotkey_may_call_llm\(\) -> bool \{\n    false\n\}/,
);

assert.match(inspector, /trigger_window_answer/);
assert.match(inspector, /get_window_preview/);
assert.match(inspector, /autoAnswer:\s*false/);
assert.doesNotMatch(inspector, /autoAnswer:\s*true/);
assert.doesNotMatch(inspector, /capture_screen_and_analyze/);
assert.match(inspector, /data-testid="open-task"/);
assert.match(inspector, /data-testid="context-debug"/);
assert.match(inspector, /Memory before/);
assert.match(inspector, /Memory after/);
assert.match(inspector, /Context update/);
assert.doesNotMatch(inspector, /get_assist_preview/);
assert.doesNotMatch(inspector, /Writing from/);

assert.match(mic, /trigger_last_you_answer/);
assert.match(mic, /sessionId:\s*null/);
assert.equal(EMPTY_YOU_NOTICE, "Say something first — Aside answers the last thing you said.");
assert.equal(
  EMPTY_ASSIST_NOTICE,
  "Nothing to send yet — talk first, then press Command+Enter.",
);

assert.match(pipeline, /window_memory::on_transcript/);
assert.match(llm, /respond_to_window/);
assert.match(llm, /WindowAnswerSink/);
assert.doesNotMatch(
  windowMod,
  /retrieve_context/,
  "window path must not call RAG",
);
assert.match(parse, /META_MARKER/);
assert.match(parse, /json_answer_so_far/);
assert.match(parse, /pub fn flush/);
assert.match(merge, /leaks_prior_suggestion/);
assert.match(prompt, /STRICT_RULES/);
assert.match(prompt, /NEW RAW CONVERSATION WINDOW/);
assert.match(prompt, /Do not assume the final sentence/);
assert.match(prompt, /CONTEXT_CHAR_BUDGET/);
assert.match(prompt, /---ASIDE_META---/);
assert.match(llm, /fn window_answer_with_claim/);
const windowAnswer = llm.slice(
  llm.indexOf("async fn window_answer_with_claim"),
  llm.indexOf("fn copy_answer_marks_into_conversation"),
);
assert.doesNotMatch(windowAnswer, /retrieve_context/, "window answer must not call RAG");
assert.doesNotMatch(windowAnswer, /prefetch_rag/, "window answer must not prefetch RAG");
assert.doesNotMatch(windowAnswer, /SplittingSink/, "window answer must not wait on Q:/A:");
assert.doesNotMatch(windowAnswer, /respond_to_question_pipelined/, "window answer is one request");
assert.match(windowMod, /pub fn warm_defaults/);
assert.match(parse, /looks_like_incomplete_fence/);

const lib = readFileSync(join(here, "../src-tauri/src/lib.rs"), "utf8");
assert.doesNotMatch(lib, /\.set_content_protected\s*\(\s*true\s*\)/);

console.log("verify-window-memory: ok");
