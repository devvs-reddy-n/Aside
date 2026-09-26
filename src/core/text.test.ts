import assert from "node:assert/strict";
import { test } from "node:test";
import { displayFileName, fileKind, isWebUrl, toBlocks, wordCount } from "./text.ts";

test("words are counted on whitespace", () => {
  assert.equal(wordCount(""), 0);
  assert.equal(wordCount("  one  two\nthree "), 3);
});

test("bullet lines become lists; blank lines split paragraphs", () => {
  const blocks = toBlocks("Intro line\n\n- first\n• second\n* third\n\nOutro");
  assert.deepEqual(blocks, [
    { kind: "para", text: "Intro line" },
    { kind: "list", items: ["first", "second", "third"] },
    { kind: "para", text: "Outro" },
  ]);
});

test("numbered lists and wrapped continuation lines are kept", () => {
  assert.deepEqual(toBlocks("1. Lead\n2) Build\n   more of build"), [{ kind: "list", items: ["Lead", "Build more of build"] }]);
});

test("file names are cleaned for display", () => {
  assert.equal(displayFileName("devendra_Resume_Senior_Call_Delivery_Analyst.pdf"), "devendra Resume Senior Call Delivery Analyst");
  assert.equal(displayFileName("notes.docx"), "notes");
  assert.equal(fileKind("a.PDF"), "PDF");
  assert.equal(fileKind("a.docx"), "Word");
  assert.equal(fileKind("pasted", "text/plain"), "Text");
});

test("only real web links count as importable", () => {
  assert.equal(isWebUrl("https://company.com/careers/role"), true);
  assert.equal(isWebUrl("company.com"), false);
  assert.equal(isWebUrl("ftp://x.com"), false);
  assert.equal(isWebUrl("https://localhost"), false);
});
