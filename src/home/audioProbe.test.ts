import test from "node:test";
import assert from "node:assert/strict";
import { runAudioProbe, type ProbeIO } from "./audioProbe.ts";

function rig() {
  const calls: string[] = [];
  const capture = { sample_rate: 48000, channels: 2, device_name: "Headphones", device_id: "chosen-output" };
  let level = (_rms: number) => {};
  let error = (_message: string) => {};
  const io: ProbeIO = {
    info: async () => ({ ok: true, value: null }),
    pinned: async () => ({ ok: true, value: "saved-output" }),
    start: async (device) => { calls.push(`capture:${device}`); return { ok: true, value: capture }; },
    stop: async () => { calls.push("stop"); },
    listen: (onLevel, onError) => { level = onLevel; error = onError; calls.push("listen"); return () => calls.push("unlisten"); },
    playTone: async (device) => { calls.push(`tone:${device}`); return { ok: true, value: undefined }; },
    wait: async () => {},
  };
  return { io, calls, capture, level: (rms: number) => level(rms), error: (message: string) => error(message) };
}

test("a played tone alone cannot mark system capture as working", async () => {
  const r = rig();
  const result = await runAudioProbe("system", r.io);
  assert.equal(result.kind, "quiet");
  assert.deepEqual(r.calls, ["listen", "capture:saved-output", "tone:chosen-output", "stop", "unlisten"]);
});

test("measured sound proves capture and the chime follows the captured output", async () => {
  const r = rig();
  r.io.playTone = async (device) => { assert.equal(device, "chosen-output"); r.level(0.03); r.level(0.03); return { ok: true, value: undefined }; };
  assert.deepEqual(await runAudioProbe("system", r.io), { kind: "heard", device: "Headphones" });
});

test("microphone checks never play a test sound", async () => {
  const r = rig();
  r.io.wait = async () => { r.level(0.02); r.level(0.02); };
  assert.equal((await runAudioProbe("mic", r.io)).kind, "heard");
  assert.ok(!r.calls.some((call) => call.startsWith("tone:")));
});

test("cancelling a check releases capture without playing a tone", async () => {
  const r = rig(); const controller = new AbortController();
  r.io.start = async () => { controller.abort(); return { ok: true, value: r.capture }; };
  assert.deepEqual(await runAudioProbe("system", r.io, { signal: controller.signal }), { kind: "cancelled" });
  assert.deepEqual(r.calls, ["listen", "stop", "unlisten"]);
});

test("a check never stops a stream already used by another call", async () => {
  const r = rig(); r.io.info = async () => ({ ok: true, value: r.capture });
  assert.equal((await runAudioProbe("system", r.io)).kind, "failed");
  assert.deepEqual(r.calls, []);
});

test("capture errors stay failures even when levels arrived", async () => {
  const r = rig(); r.io.wait = async () => { r.level(0.03); r.error("Output disconnected"); };
  assert.deepEqual(await runAudioProbe("system", r.io), { kind: "failed", message: "Output disconnected" });
  assert.deepEqual(r.calls.slice(-2), ["stop", "unlisten"]);
});

test("permission failure never plays a sound or leaves a listener", async () => {
  const r = rig(); r.io.start = async () => ({ ok: false, error: "Recording permission denied" });
  assert.deepEqual(await runAudioProbe("system", r.io), { kind: "failed", message: "Recording permission denied" });
  assert.deepEqual(r.calls, ["listen", "unlisten"]);
});

test("playback errors remain visible and are not proof of capture", async () => {
  const r = rig(); r.io.playTone = async () => ({ ok: false, error: "Output unavailable" });
  assert.deepEqual(await runAudioProbe("system", r.io), { kind: "quiet", device: "Headphones", soundError: "Output unavailable" });
});


test("two live buffers finish a long observation immediately and release its timer", async () => {
  const r = rig();
  let cancelled = false;
  r.io.wait = async (_ms, signal) => {
    await new Promise<void>((resolve) => {
      signal?.addEventListener("abort", () => { cancelled = true; resolve(); }, { once: true });
      queueMicrotask(() => { r.level(0.02); r.level(0.03); });
    });
  };
  assert.equal((await runAudioProbe("mic", r.io, { ms: 10000 })).kind, "heard");
  assert.equal(cancelled, true);
  assert.deepEqual(r.calls.slice(-2), ["stop", "unlisten"]);
});

test("one spike, invalid levels and silence cannot pass an audio check", async () => {
  const r = rig();
  r.io.wait = async () => { r.level(NaN); r.level(Infinity); r.level(-0.1); r.level(0.03); r.level(0); };
  assert.equal((await runAudioProbe("mic", r.io)).kind, "quiet");
});

test("cancelling during observation ends the wait and releases capture", async () => {
  const r = rig(); const controller = new AbortController();
  r.io.wait = async (_ms, signal) => {
    await new Promise<void>((resolve) => {
      signal?.addEventListener("abort", () => resolve(), { once: true });
      controller.abort();
    });
  };
  assert.deepEqual(await runAudioProbe("mic", r.io, { signal: controller.signal }), { kind: "cancelled" });
  assert.deepEqual(r.calls.slice(-2), ["stop", "unlisten"]);
});

test("a failed device release is visible and still removes listeners", async () => {
  const r = rig();
  r.io.wait = async () => { r.level(0.02); r.level(0.03); };
  r.io.stop = async () => ({ ok: false, error: "Device busy" });
  assert.equal((await runAudioProbe("mic", r.io)).kind, "failed");
  assert.equal(r.calls.at(-1), "unlisten");
});
