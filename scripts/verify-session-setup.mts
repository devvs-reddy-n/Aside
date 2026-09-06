import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { combineInstructions } from "../src/lib/sessionNotes.ts";

// Create → Briefing → Connect contract. Same shape as the other verify-*.mts
// scripts (this project has no frontend test runner).
// Run: npx --yes tsx scripts/verify-session-setup.mts

assert.equal(
  combineInstructions("All replies should sound spoken.", "I'm on STEM OPT, keep answers casual"),
  "All replies should sound spoken.\n\nI'm on STEM OPT, keep answers casual",
);
assert.equal(combineInstructions("keep answers casual", "keep answers casual"), "keep answers casual");
assert.equal(combineInstructions("", "I'm on STEM OPT"), "I'm on STEM OPT");
assert.equal(combineInstructions("  ", "  "), "");

const here = dirname(fileURLToPath(import.meta.url));
const setup = readFileSync(join(here, "../src/components/SessionSetup.tsx"), "utf8");

assert.doesNotMatch(setup, /joinJobAndNotes/, "instructions must not be mashed into the job post");
assert.match(setup, /type Step = "form" \| "briefing" \| "connect"/);
assert.match(setup, />\s*Continue\s*</);
assert.match(setup, /"Remember this"/);
assert.match(setup, /"Connect"/);
assert.match(setup, /setStep\("briefing"\)/);
assert.match(setup, /step === "connect"/);
assert.match(setup, /"create_session"/);
assert.match(setup, /instructions: instructions\.trim\(\) \|\| null/);
assert.doesNotMatch(
  setup,
  /sessionInstructions:/,
  "Create now sends `instructions`, the name Rust actually reads",
);
assert.match(setup, /"set_session_instructions"/);
assert.match(setup, /"commit_briefing"/);
assert.match(setup, /"start_ephemeral_session"/);
assert.match(setup, /combineInstructions\(instructions, pendingBriefing\)/);

console.log("verify-session-setup: all cases ok");
