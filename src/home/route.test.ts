import assert from "node:assert/strict";
import { test } from "node:test";
import { hrefFor, parseHash } from "./router.ts";

test("hashes parse into a page and an optional item", () => {
  assert.deepEqual(parseHash("#/copilots/abc"), { page: "copilots", param: "abc" });
  assert.deepEqual(parseHash("#/history"), { page: "history", param: null });
  assert.deepEqual(parseHash("#/settings/speech"), { page: "settings", param: "speech" });
});

test("an unknown or empty hash falls back to Copilots", () => {
  assert.equal(parseHash("").page, "copilots");
  assert.equal(parseHash("#/nonsense").page, "copilots");
});

test("links are built and encoded", () => {
  assert.equal(hrefFor("history", "a b"), "#/history/a%20b");
  assert.equal(hrefFor("resumes"), "#/resumes");
});
