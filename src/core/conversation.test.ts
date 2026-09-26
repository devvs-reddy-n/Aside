import assert from "node:assert/strict";
import { test } from "node:test";
import { conversationText, mergeConversation } from "./conversation.ts";

test("turns and suggestions are interleaved by real time", () => {
  const lines = mergeConversation(
    [
      { speaker: "them", text: "How would you scale it?", start_ms: 1000, end_ms: 2000 },
      { speaker: "you", text: "Shard by tenant.", start_ms: 9000, end_ms: 9500 },
    ],
    [{ question: "q", answer: "Use consistent hashing.", at_ms: 5000 }],
  );
  assert.deepEqual(lines.map((line) => line.at), [1000, 5000, 9000]);
});

test("the text export labels each side and flattens suggestions to one line", () => {
  const text = conversationText("Nimbus", [
    { kind: "turn", at: 1, who: "them", text: "Hi" },
    { kind: "turn", at: 2, who: "you", text: "Hello" },
    { kind: "answer", at: 3, question: "q", text: "Line one\n\nLine two" },
  ]);
  assert.deepEqual(text, ["Conversation: Nimbus", "", "Other: Hi", "Me: Hello", "Aside suggested: Line one Line two"]);
});
