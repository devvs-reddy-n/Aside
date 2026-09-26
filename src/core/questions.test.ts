import assert from "node:assert/strict";
import { test } from "node:test";
import { looksLikeQuestion } from "./questions.ts";

test("real questions are caught", () => {
  for (const text of [
    "How would you design a rate limiter?",
    "Can you walk me through your last project",
    "Tell me about a time you disagreed with your manager.",
    "Thanks for joining. What made you leave your last role?",
    "Describe how you would debug a slow query",
  ]) assert.equal(looksLikeQuestion(text), true, text);
});

test("small talk and filler are not", () => {
  for (const text of ["Okay.", "Right, great, thanks.", "Yeah", "We are a team of about forty people.", "hm"]) {
    assert.equal(looksLikeQuestion(text), false, text);
  }
});
