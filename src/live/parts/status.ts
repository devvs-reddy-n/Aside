import { useMemo } from "react";
import { useLive, useLiveUi } from "../context";
import { derivePhase, type LivePhase } from "../machine";
import { answerPages, pageIndex, type AnswerPage } from "../navigation";

export type Tone = "ok" | "info" | "warn" | "bad" | "off";

const PHASES: Record<LivePhase, { tone: Tone; text: string }> = {
  listening: { tone: "ok", text: "Listening" },
  answering: { tone: "info", text: "Answering" },
  question: { tone: "info", text: "Question" },
  offline: { tone: "bad", text: "Offline" },
  silent: { tone: "warn", text: "Audio unavailable" },
  paused: { tone: "off", text: "Paused" },
  ended: { tone: "off", text: "Ended" },
};

/** What the call is doing right now, in one word with a colour. */
export function usePhase() {
  const phase = useLive(derivePhase);
  return { phase, ...PHASES[phase] };
}

/** The question-and-answer page being shown, and where it sits in the history. */
export function useAnswerPage(): { pages: AnswerPage[]; index: number; page: AnswerPage | undefined; count: number } {
  const items = useLive((state) => state.items);
  const question = useLive((state) => state.question);
  const cursor = useLiveUi((state) => state.cursor);
  const pages = useMemo(() => answerPages({ items, question }), [items, question]);
  const index = pageIndex(pages.length, cursor?.index);
  return { pages, index, page: pages[index], count: pages.length };
}
