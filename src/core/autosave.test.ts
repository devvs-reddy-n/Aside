import assert from "node:assert/strict";
import { mock, test } from "node:test";
import { createAutosaver, type SaveState } from "./autosave.ts";

test("saves once, after the last change settles", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  const saved: string[] = [];
  const states: SaveState[] = [];
  const saver = createAutosaver(async (value: string) => { saved.push(value); return true; }, (s) => states.push(s), 600);
  saver.change("a");
  mock.timers.tick(300);
  saver.change("ab");
  mock.timers.tick(599);
  assert.deepEqual(saved, []);
  mock.timers.tick(1);
  await saver.flush();
  assert.deepEqual(saved, ["ab"]);
  assert.equal(states.at(-1), "saved");
  mock.timers.reset();
});

test("a failed save reports an error; flush saves immediately", async () => {
  const states: SaveState[] = [];
  const saver = createAutosaver(async () => false, (s) => states.push(s), 600);
  saver.change(1);
  await saver.flush();
  assert.equal(states.at(-1), "error");
  saver.dispose();
});
