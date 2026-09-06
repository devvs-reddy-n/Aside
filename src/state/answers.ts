import { useCallback, useRef, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import { LLM_EVENTS } from "../platform/eventNames";
import { cleanFinishedAnswer } from "./answerCleanup";
import type {
  AnswerDeltaEvent,
  AnswerDoneEvent,
  AnswerErrorEvent,
  QuestionCleanedEvent,
} from "../platform/types";

/** One finished question/answer pair, kept for the overlay's Prev/Next
 *  history (spec §6.2) — `AnswerCard.tsx` (live-answer) does not use this,
 *  it only reads the live fields above; purely additive. */
export interface AnswerHistoryEntry {
  question: string | null;
  cleanedQuestion: string | null;
  text: string;
  canned: boolean;
}

/** History is capped so a very long session doesn't grow this array
 *  without bound — 20 is generous for "step back a few answers", the
 *  overlay's actual use case, not a full session transcript (Past Sessions
 *  already covers that). */
const MAX_HISTORY = 20;

export interface AnswerState {
  tauri: boolean;
  /** The question currently being answered, once `answer-started` fires. */
  question: string | null;
  /** The version of the question Aside actually sent to the AI, once
   *  cleaned up (may differ slightly from `question`, e.g. trimmed filler). */
  cleanedQuestion: string | null;
  /** The answer text streamed in so far. Cleared at the start of every new
   *  question. */
  text: string;
  /** True while an answer is actively streaming in. */
  streaming: boolean;
  /** True once the last answer finished and every configured AI provider
   *  had failed, so the text shown was a stand-in message, not a real
   *  reply. */
  canned: boolean;
  error: string | null;
  /** Manually asks a question and streams the reply through the same
   *  events as an automatic answer. `sessionId` is optional context to
   *  search for relevant facts against; omit it to ask with no memory. */
  ask: (question: string, sessionId?: string | null) => Promise<boolean>;
  /** Finished answers, oldest first, most recent last — capped at
   *  {@link MAX_HISTORY}. The overlay's Prev/Next (spec §6.2) steps through
   *  this; every other consumer of this hook can ignore it. */
  history: AnswerHistoryEntry[];
}

/**
 * One suggested-answer stream: the question, the words arriving live, and
 * whether it finished normally or fell back to a canned message because
 * every AI provider failed. Mirrors `llm::respond_to_question`'s event
 * sequence exactly: `answer-started` -> zero or one `question-cleaned` ->
 * many `answer-delta` -> exactly one of `answer-done` / `answer-error`.
 */
export function useAnswerStream(): AnswerState {
  const tauri = isTauriRuntime();
  const [question, setQuestion] = useState<string | null>(null);
  const [cleanedQuestion, setCleanedQuestion] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [canned, setCanned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<AnswerHistoryEntry[]>([]);

  // Mirrors of the live fields above, read (not written) inside the
  // `answer-done` handler below — the event payload itself carries no
  // text, so the snapshot pushed into `history` has to come from whatever
  // is current at that moment, not from a value captured in a stale
  // closure. Same ref-mirror pattern `useAudioChannel` already uses.
  const questionRef = useRef<string | null>(null);
  const cleanedRef = useRef<string | null>(null);
  const textRef = useRef("");
  questionRef.current = question;
  cleanedRef.current = cleanedQuestion;
  textRef.current = text;

  useTauriEvent<{ question: string }>(LLM_EVENTS.answerStarted, (payload) => {
    setQuestion(payload.question);
    setCleanedQuestion(null);
    setText("");
    setCanned(false);
    setError(null);
    setStreaming(true);
  });
  useTauriEvent<QuestionCleanedEvent>(LLM_EVENTS.questionCleaned, (payload) => {
    setCleanedQuestion(payload.cleaned_question || null);
  });
  useTauriEvent<AnswerDeltaEvent>(LLM_EVENTS.answerDelta, (payload) => {
    setText((prev) => prev + payload.text);
  });
  useTauriEvent<AnswerDoneEvent>(LLM_EVENTS.answerDone, (payload) => {
    // Deterministic backstop on the finished text — strips a leading AI-
    // tell opener the model produced despite STRICT_RULES asking it not to,
    // and trims a dangling half-sentence if the stream ended mid-answer.
    // Skipped for a canned fallback: that's Aside's own fixed wording, not
    // model output, so there is nothing to clean. See `answerCleanup.ts`.
    const finalText = payload.canned ? textRef.current : cleanFinishedAnswer(textRef.current);
    if (finalText !== textRef.current) setText(finalText);
    setCanned(payload.canned);
    setStreaming(false);
    setHistory((prev) => {
      const entry: AnswerHistoryEntry = {
        question: questionRef.current,
        cleanedQuestion: cleanedRef.current,
        text: finalText,
        canned: payload.canned,
      };
      const next = [...prev, entry];
      return next.length > MAX_HISTORY ? next.slice(next.length - MAX_HISTORY) : next;
    });
  });
  useTauriEvent<AnswerErrorEvent>(LLM_EVENTS.answerError, (payload) => {
    setError(payload.message);
    setStreaming(false);
  });

  const ask = useCallback(async (askQuestion: string, sessionId?: string | null) => {
    setError(null);
    const result = await invoke<void>("trigger_answer", {
      question: askQuestion,
      sessionId: sessionId ?? null,
    });
    if (!result.ok) {
      setError(result.error);
      return false;
    }
    return true;
  }, []);

  return { tauri, question, cleanedQuestion, text, streaming, canned, error, ask, history };
}
