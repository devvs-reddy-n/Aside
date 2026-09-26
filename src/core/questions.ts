/**
 * Decides whether something the other person just said sounds like a
 * question aimed at you. This is a light guess done on the screen, not by
 * the AI: it only decides whether to show the "Question detected" card. It
 * never answers anything by itself. Getting it wrong costs one ignorable
 * card, so it errs toward speaking up on interview-style prompts that don't
 * end in a question mark ("Tell me about…", "Walk me through…").
 */

const OPENERS = [
  "who", "whom", "whose", "what", "when", "where", "why", "how", "which",
  "can", "could", "would", "should", "will", "shall", "do", "does", "did",
  "is", "are", "was", "were", "have", "has", "had", "may", "might",
];

const PROMPTS = [
  "tell me", "walk me through", "talk me through", "describe", "explain",
  "give me an example", "give me a", "share an example", "share a time",
  "take me through", "help me understand", "i'd like to hear", "i would like to hear",
  "i'm curious", "i am curious", "let's talk about", "let's discuss", "what about",
];

const FILLER_ONLY = /^(?:ok(?:ay)?|so|right|yeah|yes|no|mm+|uh+|um+|alright|great|cool|thanks|thank you|sure|good|nice|got it)[\s.,!?]*$/i;

export function looksLikeQuestion(raw: string): boolean {
  const text = raw.trim();
  if (text.length < 8) return false;
  if (FILLER_ONLY.test(text)) return false;
  const words = text.split(/\s+/);
  if (words.length < 3) return false;
  if (text.endsWith("?")) return true;

  // Any sentence in the turn that starts like a question counts, so
  // "Thanks for joining. How did you get into this?" still qualifies.
  const sentences = text.split(/(?<=[.!?])\s+/);
  return sentences.some((sentence) => {
    const lower = sentence.trim().toLowerCase().replace(/^[^a-z]+/, "");
    const first = lower.split(/\s+/)[0] ?? "";
    if (PROMPTS.some((prompt) => lower.startsWith(prompt))) return true;
    return OPENERS.includes(first) && lower.split(/\s+/).length >= 4;
  });
}
