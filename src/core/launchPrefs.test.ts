import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_LAUNCH, parseLaunchPrefs } from "./launchPrefs.ts";

test("nothing saved means the defaults", () => {
  assert.deepEqual(parseLaunchPrefs(null), DEFAULT_LAUNCH);
});

test("saved choice is read back", () => {
  assert.deepEqual(parseLaunchPrefs(JSON.stringify({ answersOnly: true })), { answersOnly: true });
});

test("garbage or unknown values fall back safely", () => {
  assert.deepEqual(parseLaunchPrefs("{not json"), DEFAULT_LAUNCH);
  assert.deepEqual(parseLaunchPrefs(JSON.stringify({ answersOnly: "yes" })), DEFAULT_LAUNCH);
  assert.deepEqual(parseLaunchPrefs("null"), DEFAULT_LAUNCH);
});
