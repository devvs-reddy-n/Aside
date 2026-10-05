import assert from "node:assert/strict";
import { test } from "node:test";
import type { LiveBackend } from "./backend";
import { createLiveController } from "./controller.ts";
import { answersOf, isAnswering } from "./machine.ts";

// Only provide the boundary methods exercised by each controller test.
const stubBackend = (methods: Partial<LiveBackend>): LiveBackend => methods as LiveBackend;

test("pressing Answer again while one is pending or streaming cannot send twice", async () => {
  let sent = 0;
  let finish!: () => void;
  const waiting = new Promise<void>((resolve) => { finish = resolve; });
  const controller = createLiveController(stubBackend({
    async answerWindow() {
      sent += 1;
      await waiting;
      return { ok: true, value: undefined };
    },
  }));

  const first = controller.answer();
  await controller.answer();
  assert.equal(isAnswering(controller.live.get()), true);
  controller.dispatch({ type: "answer-started", question: "Explain it.", atMs: 1 });
  finish();
  await first;
  await controller.answer();
  assert.equal(sent, 1);
  controller.dispatch({ type: "answer-done", canned: false });
  assert.equal(isAnswering(controller.live.get()), false);
});

test("a failed Answer request releases the controls so the next press is sent", async () => {
  let sent = 0;
  const controller = createLiveController(stubBackend({
    async answerWindow() {
      sent += 1;
      return { ok: false, error: "No question yet" };
    },
  }));

  await controller.answer();
  assert.equal(isAnswering(controller.live.get()), false);
  await controller.answer();
  assert.equal(sent, 2);
});

test("a rejected request releases the controls and preserves the typed follow-up", async () => {
  const controller = createLiveController(stubBackend({
    async ask() { throw new Error("Connection interrupted"); },
  }));
  assert.equal(await controller.ask("Explain that"), false);
  assert.equal(isAnswering(controller.live.get()), false);
  assert.equal(controller.live.get().notice, "Connection interrupted");
});

test("history steps through one page at a time and a new answer returns to the latest", () => {
  const controller = createLiveController(stubBackend({}));
  for (const question of ["First?", "Second?"]) {
    controller.dispatch({ type: "answer-started", question, atMs: 1 });
    controller.dispatch({ type: "answer-done", canned: false });
  }
  controller.stepAnswer(-1);
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

test("Screenshot shows the latest page at once and a capture failure becomes a visible retryable card", async () => {
  let fail!: (value: { ok: false; error: string }) => void;
  const waiting = new Promise<{ ok: false; error: string }>((resolve) => { fail = resolve; });
  const controller = createLiveController(stubBackend({ answerScreenshot: () => waiting }));
  const run = controller.screenshot();
  assert.equal(controller.ui.get().cursor, null);
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
