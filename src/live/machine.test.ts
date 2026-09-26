import assert from "node:assert/strict";
import { test } from "node:test";
import {
  answersOf,
  derivePhase,
  initialLiveState,
  isAnswering,
  reduce,
  type LiveAction,
  type LiveState,
} from "./machine.ts";

const run = (actions: LiveAction[], from: LiveState = initialLiveState(0)): LiveState =>
  actions.reduce(reduce, from);

const speak = (channel: "mic" | "system", text: string, atMs: number): LiveAction => ({
  type: "transcript", channel, text, isFinal: true, atMs,
});

test("interim words show as 'hearing' and never enter the feed", () => {
  const state = run([{ type: "transcript", channel: "system", text: "so tell me", isFinal: false, atMs: 1 }]);
  assert.deepEqual(state.interim, { who: "them", text: "so tell me" });
  assert.equal(state.items.length, 0);
});

test("a final line replaces the interim line and becomes a turn", () => {
  const state = run([
    { type: "transcript", channel: "system", text: "so tell", isFinal: false, atMs: 1 },
    speak("system", "So tell me about yourself.", 2),
  ]);
  assert.equal(state.interim, null);
  assert.equal(state.items.length, 1);
  assert.equal(state.items[0].kind === "turn" && state.items[0].who, "them");
});

test("a mic line does not clear the other side's interim words", () => {
  const state = run([
    { type: "transcript", channel: "system", text: "and then", isFinal: false, atMs: 1 },
    speak("mic", "Sure thing.", 2),
  ]);
  assert.deepEqual(state.interim, { who: "them", text: "and then" });
});

test("the same speaker talking again within a few seconds joins the same turn", () => {
  const state = run([speak("system", "Thanks for joining.", 1000), speak("system", "Let's get started.", 3000)]);
  assert.equal(state.items.length, 1);
  assert.equal(state.items[0].kind === "turn" && state.items[0].text, "Thanks for joining. Let's get started.");
});

test("a long pause, or the other speaker, starts a new turn", () => {
  const paused = run([speak("system", "One.", 0), speak("system", "Two.", 20_000)]);
  assert.equal(paused.items.length, 2);
  const swapped = run([speak("system", "Hello there.", 0), speak("mic", "Hi!", 1000)]);
  assert.equal(swapped.items.length, 2);
});

test("a question from the other side raises the question card; my own speech never does", () => {
  const detected = run([speak("system", "How would you design a rate limiter?", 0)]);
  assert.equal(detected.question?.text, "How would you design a rate limiter?");
  const mine = run([speak("mic", "How would you design a rate limiter?", 0)]);
  assert.equal(mine.question, null);
});

test("starting an answer clears the question card and labels the answer", () => {
  const state = run([
    speak("system", "Why do you want this job?", 0),
    { type: "answer-requested", source: "window", length: "small" },
    { type: "answer-started", question: "Why do you want this job?", atMs: 5 },
  ]);
  assert.equal(state.question, null);
  const [answer] = answersOf(state);
  assert.equal(answer.length, "small");
  assert.equal(answer.status, "streaming");
  assert.equal(state.nextLength, "auto");
});

test("streamed pieces add up, and a finished answer is tidied", () => {
  const state = run([
    { type: "answer-started", question: "q", atMs: 0 },
    { type: "answer-delta", text: "Certainly! I led the " },
    { type: "answer-delta", text: "migration." },
    { type: "answer-done", canned: false },
  ]);
  const [answer] = answersOf(state);
  assert.equal(answer.status, "done");
  assert.equal(answer.text, "I led the migration.");
  assert.equal(isAnswering(state), false);
});

test("a canned fallback answer is kept word for word", () => {
  const state = run([
    { type: "answer-started", question: "q", atMs: 0 },
    { type: "answer-delta", text: "Sure, I could not reach the AI." },
    { type: "answer-done", canned: true },
  ]);
  const [answer] = answersOf(state);
  assert.equal(answer.text, "Sure, I could not reach the AI.");
  assert.equal(answer.canned, true);
});

test("an answer that fails keeps its card and marks the AI unreachable on network errors", () => {
  const state = run([
    { type: "answer-started", question: "q", atMs: 0 },
    { type: "answer-error", message: "error sending request: dns error", atMs: 1 },
  ]);
  assert.equal(answersOf(state)[0].status, "error");
  assert.ok(state.offline);
  assert.equal(derivePhase(state), "offline");
});

test("a failure before any answer started still shows up as a failed card", () => {
  const state = run([{ type: "answer-error", message: "bad key", atMs: 1 }]);
  assert.equal(answersOf(state).length, 1);
  assert.equal(state.offline, null);
});

test("the next successful answer clears the offline warning", () => {
  const state = run([
    { type: "answer-error", message: "connection refused", atMs: 0 },
    { type: "answer-started", question: "q", atMs: 1 },
  ]);
  assert.equal(state.offline, null);
});

test("'no audio heard' turns a listening channel silent, and real sound recovers it", () => {
  const started = run([{ type: "channel-started", channel: "system", device: "Speakers" }]);
  const silent = reduce(started, { type: "channel-notice", channel: "system", text: "No audio heard from this device yet." });
  assert.equal(silent.channels.system.status, "silent");
  assert.equal(derivePhase(silent), "silent");
  const recovered = reduce(silent, { type: "channel-level", channel: "system", rms: 0.05 });
  assert.equal(recovered.channels.system.status, "on");
  assert.equal(recovered.channels.system.message, null);
});

test("the silence notice is ignored for a channel the person turned off", () => {
  const state = reduce(initialLiveState(0), { type: "channel-notice", channel: "system", text: "No audio heard from this device yet." });
  assert.equal(state.channels.system.status, "off");
});

test("other notices become a gentle heads-up that can be dismissed", () => {
  const state = reduce(initialLiveState(0), { type: "channel-notice", channel: "system", text: "AirPods are both output and microphone." });
  assert.ok(state.notice);
  assert.equal(reduce(state, { type: "dismiss-notice" }).notice, null);
});

test("a failed channel start is an error, and both ears off reads as paused", () => {
  const failed = reduce(initialLiveState(0), { type: "channel-failed", channel: "system", message: "denied" });
  assert.equal(failed.channels.system.status, "error");
  assert.equal(derivePhase(failed), "silent");
  const paused = run([
    { type: "channel-started", channel: "mic", device: "Mic" },
    { type: "channel-stopped", channel: "mic" },
  ]);
  assert.equal(derivePhase(paused), "paused");
});

test("ending the session produces a recap of the call", () => {
  const state = run([
    { type: "session-started", info: null, atMs: 1000 },
    speak("system", "Hello, how are you today?", 2000),
    { type: "answer-started", question: "q", atMs: 3000 },
    { type: "answer-delta", text: "Well." },
    { type: "answer-done", canned: false },
    { type: "session-ended", atMs: 61_000 },
  ]);
  assert.deepEqual(state.ended, { elapsedMs: 60_000, turns: 1, answers: 1 });
  assert.equal(derivePhase(state), "ended");
});

test("seeding the transcript only fills an empty feed", () => {
  const seeded = reduce(initialLiveState(0), { type: "seed-transcript", atMs: 5, turns: [{ channel: "system", text: "hi" }] });
  assert.equal(seeded.items.length, 1);
  const again = reduce(seeded, { type: "seed-transcript", atMs: 6, turns: [{ channel: "mic", text: "x" }] });
  assert.equal(again.items.length, 1);
});
