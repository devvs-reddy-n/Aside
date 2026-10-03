import type { Answer, DetectedQuestion, LiveState } from "./machine";

export type AnswerPage = { kind: "answer"; answer: Answer } | { kind: "question"; question: DetectedQuestion };

/** Each answered question is one page; a new unanswered question is the last page. */
export function answerPages(state: Pick<LiveState, "items" | "question">): AnswerPage[] {
  const pages: AnswerPage[] = state.items.flatMap((item) => item.kind === "answer" ? [{ kind: "answer" as const, answer: item }] : []);
  if (state.question) pages.push({ kind: "question", question: state.question });
  return pages;
}

export function pageIndex(count: number, requested?: number | null): number {
  return count === 0 ? -1 : Math.max(0, Math.min(count - 1, requested ?? count - 1));
}
