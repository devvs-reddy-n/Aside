import assert from "node:assert/strict";
import { test } from "node:test";
import type { AnswerLength } from "../bridge/api";
import type { LiveBackend } from "./backend";
import { createLiveController } from "./controller.ts";
import { answersOf, isAnswering } from "./machine.ts";

// Only provide the boundary methods exercised by each controller test.
const stubBackend = (methods: Partial<LiveBackend>): LiveBackend => methods as LiveBackend;

test("each length action sends one override; the next Answer always returns to auto", async () => {
  const requests: (AnswerLength | null)[] = [];
  const controller = createLiveController(stubBackend({
    async answerWindow(length) {
      requests.push(length);
      controller.dispatch({ type: "answer-started", question: "How does it work?", atMs: requests.length });
      controller.dispatch({ type: "answer-done", canned: false });
      return { ok: true, value: undefined };
    },
  }));

  for (const length of ["small", "medium", "long"] as const) {
    await controller.answer(length);
    assert.equal(answersOf(controller.live.get()).at(-1)?.length, length);

    await controller.answer();
    assert.equal(answersOf(controller.live.get()).at(-1)?.length, "auto");
  }
  assert.deepEqual(requests, ["small", null, "medium", null, "long", null]);
});

test("pressing another length while an answer is pending or streaming cannot send twice", async () => {
  const requests: (AnswerLength | null)[] = [];
  let finish!: () => void;
  const waiting = new Promise<void>((resolve) => { finish = resolve; });
  const controller = createLiveController(stubBackend({
    async answerWindow(length) {
      requests.push(length);
      await waiting;
      return { ok: true, value: undefined };
    },
  }));

  const first = controller.answer("small");
  await controller.answer("long");
  assert.equal(isAnswering(controller.live.get()), true);
  controller.dispatch({ type: "answer-started", question: "Explain it.", atMs: 1 });
  finish();
  await first;
  await controller.answer();
  assert.deepEqual(requests, ["small"]);
  controller.dispatch({ type: "answer-done", canned: false });
  assert.equal(isAnswering(controller.live.get()), false);
});

test("a failed length request releases the controls and does not affect the next Answer", async () => {
  const requests: (AnswerLength | null)[] = [];
  const controller = createLiveController(stubBackend({
    async answerWindow(length) {
      requests.push(length);
      return { ok: false, error: "No question yet" };
    },
  }));

  await controller.answer("long");
  assert.equal(isAnswering(controller.live.get()), false);
  await controller.answer();
  assert.deepEqual(requests, ["long", null]);
});

test("an ignored length request leaves the next keyboard-triggered answer automatic", async () => {
  const controller = createLiveController(stubBackend({
    async answerWindow() { return { ok: true, value: undefined }; },
  }));
  await controller.answer("long");
  assert.equal(isAnswering(controller.live.get()), false);
  controller.dispatch({ type: "answer-started", question: "Next question", atMs: 1 });
  assert.equal(answersOf(controller.live.get())[0].length, "auto");
});

test("a rejected request releases the controls and preserves the typed follow-up", async () => {
  const controller = createLiveController(stubBackend({
    async ask() { throw new Error("Connection interrupted"); },
  }));
  assert.equal(await controller.ask("Explain that"), false);
  assert.equal(isAnswering(controller.live.get()), false);
  assert.equal(controller.live.get().notice, "Connection interrupted");
});

test("Answer view steps through one page at a time and a new answer returns to the latest", () => {
  const controller = createLiveController(stubBackend({}));
  controller.ui.set({ view: "all" });
  for (const question of ["First?", "Second?"]) {
    controller.dispatch({ type: "answer-started", question, atMs: 1 });
    controller.dispatch({ type: "answer-done", canned: false });
  }
  controller.stepAnswer(-1);
  assert.equal(controller.ui.get().view, "answers");
  assert.equal(controller.ui.get().cursor?.index, 0);
  controller.stepAnswer(-1);
  assert.equal(controller.ui.get().cursor?.index, 0);
  controller.stepAnswer(1);
  assert.equal(controller.ui.get().cursor?.index, 1);
  controller.dispatch({ type: "transcript", channel: "system", text: "What about a third option?", isFinal: true, atMs: 4 });
  assert.equal(controller.ui.get().cursor, null);
  controller.stepAnswer(-1);
  assert.equal(controller.ui.get().cursor?.index, 1);
  controller.dispatch({ type: "answer-started", question: "Third?", atMs: 5 });
  assert.equal(controller.ui.get().cursor, null);
});

test("the conversation thread steps through answers in place instead of switching to the Answer view", async () => {
  const controller = createLiveController(stubBackend({ answerScreenshot: async () => ({ ok: true, value: undefined }) }));
  controller.ui.set({ layout: "thread", view: "all" });
  for (const question of ["First?", "Second?"]) {
    controller.dispatch({ type: "answer-started", question, atMs: 1 });
    controller.dispatch({ type: "answer-done", canned: false });
  }
  controller.stepAnswer(-1);
  assert.equal(controller.ui.get().view, "all");
  assert.equal(controller.ui.get().cursor?.index, 0);
  await controller.screenshot();
  assert.equal(controller.ui.get().view, "all");
});

test("changing layout forgets the answer being stepped to", () => {
  const controller = createLiveController(stubBackend({}));
  controller.dispatch({ type: "answer-started", question: "First?", atMs: 1 });
  controller.dispatch({ type: "answer-done", canned: false });
  controller.stepAnswer(-1);
  controller.setLayout("prompter");
  assert.equal(controller.ui.get().layout, "prompter");
  assert.equal(controller.ui.get().cursor, null);
});

test("Screenshot opens Answer mode immediately and a capture failure becomes a visible retryable card", async () => {
  let fail!: (value: { ok: false; error: string }) => void;
  const waiting = new Promise<{ ok: false; error: string }>((resolve) => { fail = resolve; });
  const controller = createLiveController(stubBackend({ answerScreenshot: () => waiting }));
  const run = controller.screenshot();
  assert.equal(controller.ui.get().view, "answers");
  assert.equal(controller.ui.get().screenshotBusy, true);
  assert.equal(controller.live.get().nextSource, "screen");
  fail({ ok: false, error: "Screen Recording access is needed" });
  await run;
  const failed = answersOf(controller.live.get()).at(-1)!;
  assert.equal(failed.source, "screen");
  assert.equal(failed.status, "error");
  assert.match(failed.error!, /Screen Recording/);
  assert.equal(controller.ui.get().screenshotBusy, false);
  assert.equal(isAnswering(controller.live.get()), false);
});
