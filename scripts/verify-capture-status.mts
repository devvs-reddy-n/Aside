import assert from "node:assert/strict";
import {
  EMPTY_STATUS,
  anyChannelListening,
  describeChannel,
  failuresFromReport,
  mergeCaptureStatus,
  type CaptureStatus,
} from "../src/lib/captureStatus.ts";

// Pure frontend logic checks for the capture-status strip — the surface
// added so the app can never again be silently deaf with nothing on screen
// explaining why. Same shape as verify-answer-format.mts (this project has
// no frontend test runner).
// Run: npx --yes tsx scripts/verify-capture-status.mts

const listening: CaptureStatus = {
  mic_capturing: true,
  mic_transcribing: true,
  system_capturing: true,
  system_transcribing: true,
};

// --- describeChannel ---

{
  const line = describeChannel("you", listening);
  assert.equal(line.label, "Me");
  assert.equal(line.tone, "ok");
  assert.equal(line.text, "listening");
  assert.equal(line.canRetry, false, "a working channel offers no retry");
}

{
  // The exact failure the old UI made easy to land in: sound arriving but
  // no words appearing. It gets its own message, not a generic red dot.
  const halfway = { ...listening, system_transcribing: false };
  const line = describeChannel("others", halfway);
  assert.equal(line.tone, "warn");
  assert.match(line.text, /not turning it into words/);
  assert.equal(line.canRetry, true);
}

{
  const line = describeChannel("others", EMPTY_STATUS);
  assert.equal(line.label, "Others");
  assert.equal(line.tone, "bad");
  assert.equal(line.text, "not listening");
  assert.equal(line.canRetry, true);
}

{
  // A real reason always beats the generic message.
  const line = describeChannel("others", EMPTY_STATUS, "that speaker can't be listened to");
  assert.equal(line.tone, "bad");
  assert.equal(line.text, "that speaker can't be listened to");
  assert.equal(line.canRetry, true);
}

{
  // Plain-language rule: no internal vocabulary in anything shown on screen.
  for (const who of ["you", "others"] as const) {
    for (const status of [listening, EMPTY_STATUS, { ...listening, mic_transcribing: false }]) {
      const { text } = describeChannel(who, status);
      for (const jargon of ["STT", "transcrib", "channel", "invoke", "engine"]) {
        assert.ok(!text.includes(jargon), `"${text}" leaks the word "${jargon}"`);
      }
    }
  }
}

// --- failuresFromReport ---

assert.deepEqual(failuresFromReport(null), {});

assert.deepEqual(
  failuresFromReport({ mic: { state: "started" }, system: { state: "started" } }),
  {},
  "a fully successful start reports no failures",
);

{
  // The case that matters most: system audio failing must not hide a
  // perfectly working mic.
  const out = failuresFromReport({
    mic: { state: "started" },
    system: { state: "failed", reason: "that speaker can't be listened to" },
  });
  assert.equal(out.mic, undefined);
  assert.equal(out.system, "that speaker can't be listened to");
}

{
  const out = failuresFromReport({
    mic: { state: "failed", reason: "microphone is off" },
    system: { state: "failed", reason: "no speaker found" },
  });
  assert.equal(out.mic, "microphone is off");
  assert.equal(out.system, "no speaker found");
}

// --- mergeCaptureStatus ---

assert.deepEqual(mergeCaptureStatus(EMPTY_STATUS, { mic_capturing: true }), {
  ...EMPTY_STATUS,
  mic_capturing: true,
});

assert.deepEqual(
  mergeCaptureStatus(listening, {}),
  listening,
  "an empty update must not clear fields it didn't carry",
);

// --- anyChannelListening ---

assert.equal(anyChannelListening(listening), true);
assert.equal(anyChannelListening(EMPTY_STATUS), false);
assert.equal(
  anyChannelListening({ ...EMPTY_STATUS, mic_capturing: true }),
  false,
  "capturing without producing words is not 'listening'",
);
assert.equal(
  anyChannelListening({ ...EMPTY_STATUS, mic_capturing: true, mic_transcribing: true }),
  true,
  "one fully working channel is enough",
);

console.log("verify-capture-status: all cases ok");
