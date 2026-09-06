import assert from "node:assert/strict";
import { formatAnswer } from "../src/lib/answerFormat.ts";

// Pure frontend logic checks for the Parakeet-style answer card splitter.
// Run: npx --yes tsx scripts/verify-answer-format.mts

assert.deepEqual(formatAnswer(""), { prose: "", steps: [], code: null });

{
  const r = formatAnswer("Plain spoken answer with no structure.");
  assert.equal(r.prose, "Plain spoken answer with no structure.");
  assert.equal(r.steps.length, 0);
  assert.equal(r.code, null);
}

{
  const r = formatAnswer(`I'll walk left to right.

Key steps:
- Map symbols
- Subtract when smaller
- Add otherwise

\`\`\`python
def roman_to_int(s):
    return 0
\`\`\``);
  assert.match(r.prose, /walk left to right/);
  assert.deepEqual(r.steps, ["Map symbols", "Subtract when smaller", "Add otherwise"]);
  assert.ok(r.code);
  assert.equal(r.code.language, "python");
  assert.match(r.code.source, /def roman_to_int/);
}

{
  const r = formatAnswer(`Intro

- one thing
- two thing
- three thing`);
  // Ordinary bullets must stay in prose — never invent a Key steps section.
  assert.equal(r.steps.length, 0);
  assert.match(r.prose, /one thing/);
}

{
  const r = formatAnswer("Only one - bullet here is not enough");
  assert.equal(r.steps.length, 0);
  assert.match(r.prose, /Only one/);
}

console.log("verify-answer-format: 5 cases ok");
