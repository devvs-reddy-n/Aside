import assert from "node:assert/strict";
import { test } from "node:test";
import { describeAnswerError, looksLikePermissionProblem } from "./errors.ts";

test("bad keys point at Settings", () => {
  const friendly = describeAnswerError("OpenAI returned 401: Incorrect API key provided");
  assert.equal(friendly.settings, true);
});

test("connection problems say the AI can't be reached", () => {
  assert.match(describeAnswerError("error sending request: dns error").title, /reach/i);
});

test("unknown failures keep a short raw message as the hint", () => {
  assert.equal(describeAnswerError("weird thing").hint, "weird thing");
});

test("permission wording is recognised, and a missing reason defaults to permission", () => {
  assert.equal(looksLikePermissionProblem("System audio permission denied"), true);
  assert.equal(looksLikePermissionProblem(null), true);
  assert.equal(looksLikePermissionProblem("device unplugged"), false);
});
