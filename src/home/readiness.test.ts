import assert from "node:assert/strict";
import { test } from "node:test";
import type { CopilotSummary } from "../bridge/types";
import { blockers, checksFor, type SystemSnapshot } from "./readiness.ts";

const ready: SystemSnapshot = {
  engine: "speechmatics", speechKey: true, parakeetReady: false,
  provider: "open_ai", model: "gpt-5.6-luna", aiKey: true, hidden: true,
};

const copilot = { has_resume: true, document_count: 2 } as CopilotSummary;

test("a fully set up system has nothing blocking", () => {
  assert.equal(blockers(checksFor(ready, copilot)).length, 0);
});

test("a cloud engine without a key blocks and points to Settings", () => {
  const checks = checksFor({ ...ready, speechKey: false }, copilot);
  const speech = checks.find((check) => check.id === "speech");
  assert.equal(speech?.status, "todo");
  assert.equal(speech?.fix?.page, "settings");
});

test("Parakeet blocks until its model is downloaded, and needs no key", () => {
  const notYet = checksFor({ ...ready, engine: "parakeet_local", speechKey: false }, null);
  assert.equal(notYet.find((c) => c.id === "speech")?.status, "todo");
  const done = checksFor({ ...ready, engine: "parakeet_local", speechKey: false, parakeetReady: true }, null);
  assert.equal(done.find((c) => c.id === "speech")?.status, "ok");
});

test("a missing AI key blocks", () => {
  assert.equal(blockers(checksFor({ ...ready, aiKey: false }, null)).map((c) => c.id).join(), "ai");
});

test("a missing resume is only a hint, and permissions are never shown as ticked", () => {
  const checks = checksFor(ready, { has_resume: false, document_count: 0 } as CopilotSummary);
  assert.equal(checks.find((c) => c.id === "sources")?.status, "info");
  assert.equal(checks.find((c) => c.id === "permissions")?.status, "info");
});
