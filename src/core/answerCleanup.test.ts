import assert from "node:assert/strict";
import { test } from "node:test";
import { cleanFinishedAnswer, tightenAnswer } from "./answerCleanup.ts";

test("a chatty opener is removed and the next word is capitalised", () => {
  assert.equal(tightenAnswer("Certainly! I led the migration."), "I led the migration.");
});

test("sentence-initial filler is removed", () => {
  assert.equal(tightenAnswer("I built it. Furthermore, it scaled."), "I built it. It scaled.");
});

test("doubled spaces in prose collapse, but code indentation is left alone", () => {
  const input = "Here  is the code.\n```python\ndef f(x):\n    return x\n```";
  assert.equal(tightenAnswer(input), "Here is the code.\n```python\ndef f(x):\n    return x\n```");
});

test("an unfinished code block keeps its indentation too", () => {
  const input = "Try this:\n```python\nif ok:\n    run()";
  assert.equal(tightenAnswer(input), input);
});

test("a dangling half-sentence is trimmed away", () => {
  assert.equal(cleanFinishedAnswer("It worked well. Then we starte"), "It worked well.");
});
