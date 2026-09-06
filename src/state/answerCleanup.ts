/**
 * Deterministic backstop for the finished answer text, applied once
 * streaming ends. `STRICT_RULES` (`window_memory/prompt.rs`) already asks
 * the model not to open with "Absolutely," "Certainly," or "Great
 * question" (rule 34) and to end on a real sentence — but a prompt is a
 * strong nudge, not a lock, and both reference apps in `references/`
 * treat this the same way: the prompt shapes the model's own output, and a
 * small, pure, idempotent pass on the finished text is the actual
 * guarantee behind it. Adapted from `references/src/shared/humanize.ts`'s
 * `tightenAnswer` and `references/src/shared/answerLength.ts`'s
 * `trimDanglingTail` — same idea, rewritten for Aside's own answer shape
 * (which can contain a fenced code block or a "Key Steps" bullet list,
 * `layers/overlay/answerParse.ts`), not copied line for line.
 *
 * Never called on a canned fallback message (`useAnswerStream`'s own
 * fixed wording) — only on real model output.
 */

/**
 * A chatty acknowledgement at the very start ("Certainly! …", "Sure, …").
 * Requires trailing punctuation, not just a space, so a real sentence that
 * happens to start with one of these words is never touched — "Sure
 * enough, the test failed" keeps its meaning; only "Sure, …" / "Sure
 * thing! …" style acknowledgements are stripped. Longer phrases are listed
 * before their prefixes so alternation can't half-match.
 */
const OPENER_RE =
  /^(?:certainly|sure thing|sure|of course|absolutely|great question|good question|got it|no problem|no worries|happy to help)\s*[!.,:;—–-]+\s+/i;

/**
 * Sentence-initial filler that adds nothing — the same "AI essay" tells
 * `STRICT_RULES`'s spoken-language rules (30-35) already push against.
 * Stripped only at a sentence boundary (start of text, or after terminal
 * punctuation), so mid-sentence uses are never touched.
 */
const FILLERS: readonly string[] = [
  "Furthermore",
  "Moreover",
  "Additionally",
  "In addition",
  "In conclusion",
  "In summary",
  "To summarize",
  "To sum up",
  "That said",
  "That being said",
  "It['’]?s worth noting that",
  "It is worth noting that",
  "It['’]?s important to note that",
  "It is important to note that",
  "It should be noted that",
];

const FILLER_RE = new RegExp(
  `(^|[.!?]["'”’)\\]]*\\s+)(?:${FILLERS.join("|")}),?\\s+(\\S)`,
  "gi",
);

const MAX_FILLER_PASSES = 4;

/**
 * Strips a leading AI-tell opener and any sentence-initial filler from a
 * finished answer. Pure and idempotent; never returns an empty string for
 * a non-empty input (an opener-only reply like "Sure!" survives unchanged
 * rather than vanishing).
 */
export function tightenAnswer(text: string): string {
  if (!text) return text;

  let out = text.replace(OPENER_RE, "");
  if (out !== text && out) out = out.charAt(0).toUpperCase() + out.slice(1);

  // Chained fillers ("Moreover, furthermore, …") need repeat passes — each
  // strip can expose the next filler sitting at the new boundary. Bounded,
  // converges (each pass strictly shortens the text or stops).
  for (let i = 0; i < MAX_FILLER_PASSES; i++) {
    const next = out.replace(FILLER_RE, (_match, pre: string, ch: string) => pre + ch.toUpperCase());
    if (next === out) break;
    out = next;
  }

  out = out.replace(/[ \t]{2,}/g, " ").trimStart();
  return out || text.trim();
}

/** How much of a trailing fragment is worth trimming — anything longer is
 *  more likely a real, complete thought the sentence-end regex just
 *  missed (e.g. an abbreviation) than a genuine mid-sentence cutoff. */
const DANGLING_TAIL_MAX_CHARS = 160;

const SENTENCE_END_RE = /[.!?]["'”’)\]]*(?:\s+|$)/g;

/**
 * If the stream ended mid-sentence — a dropped connection, a provider's
 * own token cap, a mid-stream error after some text already arrived —
 * trims back to the last complete sentence instead of shipping a dangling
 * fragment. Bails out (returns the text unchanged) whenever trimming could
 * plausibly mangle real structure: a fenced code block or a "Key Steps"
 * bullet line always contains a newline, so any dangling tail with one is
 * left alone rather than risking a cut mid-list or mid-code.
 */
export function trimDanglingTail(text: string): string {
  if (!text) return text;
  const trimmed = text.trimEnd();
  if (/[.!?]["'”’)\]]*$/.test(trimmed)) return text;

  let lastEnd = -1;
  for (const match of trimmed.matchAll(SENTENCE_END_RE)) {
    lastEnd = match.index + match[0].length;
  }
  if (lastEnd === -1) return text;

  const tail = trimmed.slice(lastEnd);
  if (tail.length === 0 || tail.length > DANGLING_TAIL_MAX_CHARS) return text;
  if (tail.includes("\n") || tail.includes("`")) return text;

  return trimmed.slice(0, lastEnd).trimEnd();
}

/** Both passes, in order: strip AI tells first (they sit at the very start
 *  or after a sentence boundary), then trim a dangling tail (which only
 *  matters at the very end) — the two never interact. */
export function cleanFinishedAnswer(text: string): string {
  return trimDanglingTail(tightenAnswer(text));
}
