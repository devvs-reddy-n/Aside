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

test("flush waits for an in-flight save even after its queue is consumed", async () => {
  let finish!: (ok: boolean) => void;
  const saver = createAutosaver(() => new Promise<boolean>((resolve) => { finish = resolve; }), () => {}, 60_000);
  saver.change("latest");
  const first = saver.flush();
  await Promise.resolve();
  let settled = false;
  const second = saver.flush().then((ok) => { settled = true; return ok; });
  await Promise.resolve();
  assert.equal(settled, false);
  finish(true);
  assert.equal(await first, true);
  assert.equal(await second, true);
  saver.dispose();
});

test("a failed save blocks flush and the next explicit flush retries the same edits", async () => {
  const values: string[] = [];
  const saver = createAutosaver(async (value: string) => { values.push(value); return values.length > 1; }, () => {}, 60_000);
  saver.change("must not lose this");
  assert.equal(await saver.flush(), false);
  assert.equal(await saver.flush(), true);
  assert.deepEqual(values, ["must not lose this", "must not lose this"]);
  saver.dispose();
});

test("flush drains newer edits before reporting success", async () => {
  let finish!: (ok: boolean) => void;
  const values: string[] = [];
  const saver = createAutosaver((value: string) => {
    values.push(value);
    return values.length === 1 ? new Promise<boolean>((resolve) => { finish = resolve; }) : Promise.resolve(true);
  }, () => {}, 60_000);
  saver.change("older");
  const saving = saver.flush();
  await Promise.resolve();
  saver.change("newest");
  finish(true);
  assert.equal(await saving, true);
  assert.deepEqual(values, ["older", "newest"]);
  saver.dispose();
});


test("concurrent flush calls both wait for the newest queued save", async () => {
  const finishes: Array<(ok: boolean) => void> = [];
  const saver = createAutosaver(() => new Promise<boolean>((resolve) => { finishes.push(resolve); }), () => {}, 60_000);
  saver.change("older");
  const first = saver.flush();
  await Promise.resolve();
  let settled = false;
  const second = saver.flush().then((ok) => { settled = true; return ok; });
  saver.change("newest");
  finishes[0](true);
  for (let step = 0; step < 20 && finishes.length < 2; step++) await Promise.resolve();
  assert.equal(finishes.length, 2);
  assert.equal(settled, false);
  finishes[1](false);
  assert.equal(await first, false);
  assert.equal(await second, false);
  saver.dispose();
});
