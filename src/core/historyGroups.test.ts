import assert from "node:assert/strict";
import test from "node:test";
import { dayLabel, groupByDay, isShort } from "./historyGroups.ts";

const now = new Date(2026, 8, 25, 15, 0).getTime();
const at = (day: number, hour: number) => new Date(2026, 8, day, hour, 0).getTime();

test("calls group under Today, Yesterday, then their date, newest first", () => {
  const groups = groupByDay([
    { id: "a", started_at_ms: at(25, 9) },
    { id: "b", started_at_ms: at(24, 18) },
    { id: "c", started_at_ms: at(25, 13) },
    { id: "d", started_at_ms: at(20, 10) },
  ], now);
  assert.deepEqual(groups.map((g) => g.label.startsWith("Today") || g.label.startsWith("Yesterday") ? g.label : "date"), ["Today", "Yesterday", "date"]);
  assert.deepEqual(groups[0].items.map((s) => s.id), ["c", "a"]);
  assert.equal(groups[2].items[0].id, "d");
});

test("just after midnight, last night's calls are Yesterday", () => {
  const justAfter = new Date(2026, 8, 25, 0, 5).getTime();
  assert.equal(dayLabel(at(24, 23), justAfter), "Yesterday");
  assert.equal(dayLabel(new Date(2025, 0, 3).getTime(), now).includes("2025"), true);
});

test("only finished calls under ten seconds count as short", () => {
  assert.equal(isShort({ duration_ms: 4_000, ended_at_ms: 1 }), true);
  assert.equal(isShort({ duration_ms: 10_000, ended_at_ms: 1 }), false);
  assert.equal(isShort({ duration_ms: null, ended_at_ms: null }), false);
  assert.equal(isShort({ duration_ms: null, ended_at_ms: 1 }), true);
});
