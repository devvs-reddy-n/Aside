import type { SessionAnswer, TranscriptTurn } from "../bridge/types";

export type ConversationLine =
  | { kind: "turn"; at: number; who: "you" | "them"; text: string }
  | { kind: "answer"; at: number; question: string; text: string };

/** Spoken turns and Aside's suggestions in the order they happened. Both carry real clock times. */
export function mergeConversation(turns: TranscriptTurn[], answers: SessionAnswer[]): ConversationLine[] {
  const lines: ConversationLine[] = [
    ...turns.map((turn): ConversationLine => ({ kind: "turn", at: turn.start_ms, who: turn.speaker, text: turn.text })),
    ...answers.map((answer): ConversationLine => ({ kind: "answer", at: answer.at_ms, question: answer.question, text: answer.answer })),
  ];
  return lines.sort((a, b) => a.at - b.at);
}

/** Plain-text lines for the Text / PDF download of a call. */
export function conversationText(title: string, lines: ConversationLine[]): string[] {
  return [
    `Conversation: ${title}`,
    "",
    ...lines.map((line) =>
      line.kind === "turn"
        ? `${line.who === "you" ? "Me" : "Other"}: ${line.text}`
        : `Aside suggested: ${line.text.replace(/\s*\n+\s*/g, " ")}`,
    ),
  ];
}
