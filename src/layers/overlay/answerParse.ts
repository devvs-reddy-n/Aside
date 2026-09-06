/**
 * Splits a streamed answer's plain text into the spec's three optional
 * pieces — a fenced code block, a "Key Steps" bullet list, and whatever
 * prose is left — for `AnswerCard.tsx` to render as the spec §6.3 sections
 * (💬 Question / ⭐ Answer / 🔑 Key Steps / `</>` Code).
 *
 * **Scoping note**: this is real parsing of whatever the model actually
 * returned, not a fabricated structure. Key Steps only ever appears when the
 * text itself contains a bullet list sitting under something that reads
 * like a "Steps" heading — if that signal isn't there, `keySteps` comes back
 * `null` and no section renders. The backend prompt (`STRICT_RULES`) does
 * not force this shape for every answer — it only renders when the model
 * happens to structure its own answer that way.
 */

export interface ParsedCodeBlock {
  language: string | null;
  code: string;
}

export interface ParsedAnswer {
  /** The answer's prose, with any Key Steps block and code fence removed. */
  prose: string;
  /** `null` unless the text contains a real bullet list under a "steps"-
   *  shaped heading — never fabricated. */
  keySteps: string[] | null;
  /** `null` unless the text contains at least one fenced code block
   *  (```lang\n...\n```). Only the first fence is used — spec §6.4 only
   *  ever shows one code panel per answer. */
  code: ParsedCodeBlock | null;
}

const CODE_FENCE_RE = /```([a-zA-Z0-9_+-]*)\n?([\s\S]*?)```/;

// Conservative on purpose: only a heading that actually reads as "steps",
// immediately followed by two or more bullet lines, counts. A plain bullet
// list elsewhere in the prose (reasons, a list of tools, ...) is left in
// the Answer prose untouched — turning every bullet list in the app into a
// "Key Steps" card would misrepresent what the model actually said.
const STEPS_BLOCK_RE = /(^|\n)[ \t]*(?:key steps|steps)\s*[:：]?[ \t]*\n((?:[ \t]*[-*•][^\n]*\n?){2,})/i;

function stripBullet(line: string): string {
  return line.replace(/^[ \t]*[-*•]\s*/, "").trim();
}

export function parseAnswerText(raw: string): ParsedAnswer {
  let text = raw;
  let code: ParsedCodeBlock | null = null;

  const fenceMatch = CODE_FENCE_RE.exec(text);
  if (fenceMatch) {
    const language = fenceMatch[1]?.trim() || null;
    const body = fenceMatch[2].replace(/\n$/, "");
    code = { language, code: body };
    text = (text.slice(0, fenceMatch.index) + text.slice(fenceMatch.index + fenceMatch[0].length)).trim();
  }

  let keySteps: string[] | null = null;
  const stepsMatch = STEPS_BLOCK_RE.exec(text);
  if (stepsMatch) {
    const lines = stepsMatch[2]
      .split("\n")
      .map(stripBullet)
      .filter((line) => line.length > 0);
    if (lines.length >= 2) {
      keySteps = lines;
      text = (text.slice(0, stepsMatch.index) + text.slice(stepsMatch.index + stepsMatch[0].length)).trim();
    }
  }

  return { prose: text.trim(), keySteps, code };
}
