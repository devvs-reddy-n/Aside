import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

/**
 * Every stylesheet is merged into one file in the real app, so a single
 * unbalanced brace in one file can silently wipe out the colours and fonts
 * of the whole app (it happened once). The dev preview keeps files separate
 * and hides the problem, so this check runs on every file instead.
 */
function cssFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return cssFiles(path);
    return name.endsWith(".css") ? [path] : [];
  });
}

test("every stylesheet has balanced braces and closed comments", () => {
  const files = cssFiles("src");
  assert.ok(files.length > 0);
  for (const file of files) {
    const raw = readFileSync(file, "utf8");
    assert.equal((raw.match(/\/\*/g) ?? []).length, (raw.match(/\*\//g) ?? []).length, `${file}: unclosed comment`);
    const text = raw.replace(/\/\*[\s\S]*?\*\//g, "").replace(/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/g, "");
    let depth = 0;
    text.split("\n").forEach((line, index) => {
      for (const ch of line) {
        if (ch === "{") depth += 1;
        if (ch === "}") depth -= 1;
        assert.ok(depth >= 0, `${file}:${index + 1}: stray "}"`);
      }
    });
    assert.equal(depth, 0, `${file}: a "{" is never closed`);
  }
});
