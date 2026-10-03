import assert from "node:assert/strict";
import { test } from "node:test";
import { answerPages, pageIndex } from "./navigation.ts";
import { initialLiveState, reduce } from "./machine.ts";

test("Answer pages exclude conversation turns, retain previous responses and include the new question", () => {
  let state = initialLiveState(0);
  state = reduce(state, { type: "answer-started", question: "Old question", atMs: 1 });
  state = reduce(state, { type: "answer-done", canned: false });
  state = reduce(state, { type: "transcript", channel: "system", text: "How does this work?", isFinal: true, atMs: 2 });
  const pages = answerPages(state);
  assert.deepEqual(pages.map((page) => page.kind), ["answer", "question"]);
  assert.equal(pageIndex(pages.length), 1);
  assert.equal(pageIndex(pages.length, -1), 0);
  assert.equal(pageIndex(pages.length, 99), 1);
  assert.equal(pageIndex(0), -1);
});
