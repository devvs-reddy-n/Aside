import type { ActiveSessionInfo, LiveTranscriptTurn } from "../bridge/types";
import { cleanFinishedAnswer } from "../core/answerCleanup.ts";
import { looksLikeQuestion } from "../core/questions.ts";

/**
 * Everything the live window knows, and the only rules for changing it.
 * This file has no screen code and no calls to the app's backend, so every
 * rule can be tested on its own (`machine.test.ts`). Events from the
 * backend and button presses are both turned into an `Action` and passed
 * through `reduce`.
 */

export type ChannelKey = "mic" | "system";
/** off = you turned it off · starting = warming up · on = listening
 *  silent = on, but nothing heard yet (a normal quiet state) · error = failed */
export type ChannelStatus = "off" | "starting" | "on" | "silent" | "error";
export type AnswerSource = "window" | "screen" | "typed";
export type AnswerLengthChoice = "auto" | "small" | "medium" | "long";

export interface ChannelView {
  status: ChannelStatus;
  level: number;
  speaking: boolean;
  device: string | null;
  /** The reason behind `error` or `silent`, in the backend's own words. */
  message: string | null;
}

export interface Turn {
  kind: "turn";
  id: string;
  who: "me" | "them";
  text: string;
  atMs: number;
}

export interface Answer {
  kind: "answer";
  id: string;
  question: string;
  cleanedQuestion: string | null;
  text: string;
  status: "streaming" | "done" | "error";
  /** True when every AI provider failed and the text is Aside's own fallback message. */
  canned: boolean;
  error: string | null;
  source: AnswerSource;
  length: AnswerLengthChoice;
  atMs: number;
}

export type FeedItem = Turn | Answer;

export interface DetectedQuestion {
  turnId: string;
  text: string;
}

export interface Recap {
  elapsedMs: number;
  turns: number;
  answers: number;
}

export interface LiveState {
  session: ActiveSessionInfo | null;
  startedAtMs: number;
  items: FeedItem[];
  /** The words being spoken right now, before the speech engine settles on them. */
  interim: { who: "me" | "them"; text: string } | null;
  channels: Record<ChannelKey, ChannelView>;
  question: DetectedQuestion | null;
  /** Set when the AI service could not be reached. */
  offline: string | null;
  /** A gentle one-line heads-up (never an error). */
  notice: string | null;
  /** A button request is waiting for the backend to start or finish. */
  requestPending: boolean;
  nextSource: AnswerSource;
  nextLength: AnswerLengthChoice;
  ended: Recap | null;
  counter: number;
}

export type LiveAction =
  | { type: "session-started"; info: ActiveSessionInfo | null; atMs: number }
  | { type: "seed-transcript"; turns: LiveTranscriptTurn[]; atMs: number }
  | { type: "transcript"; channel: ChannelKey; text: string; isFinal: boolean; atMs: number }
  | { type: "answer-requested"; source: AnswerSource; length: AnswerLengthChoice }
  | { type: "answer-request-settled" }
  | { type: "answer-started"; question: string; atMs: number }
  | { type: "answer-cleaned"; cleaned: string }
  | { type: "answer-delta"; text: string }
  | { type: "answer-done"; canned: boolean }
  | { type: "answer-error"; message: string; atMs: number }
  | { type: "trigger-failed"; message: string }
  | { type: "notice"; text: string }
  | { type: "channel-starting"; channel: ChannelKey }
  | { type: "channel-started"; channel: ChannelKey; device: string | null }
  | { type: "channel-stopped"; channel: ChannelKey }
  | { type: "channel-failed"; channel: ChannelKey; message: string }
  | { type: "channel-level"; channel: ChannelKey; rms: number }
  | { type: "channel-vad"; channel: ChannelKey; speaking: boolean }
  | { type: "channel-notice"; channel: ChannelKey; text: string }
  | { type: "dismiss-notice" }
  | { type: "dismiss-question" }
  | { type: "session-ended"; atMs: number };

const emptyChannel = (): ChannelView => ({ status: "off", level: 0, speaking: false, device: null, message: null });

export function initialLiveState(now = Date.now()): LiveState {
  return {
    session: null,
    startedAtMs: now,
    items: [],
    interim: null,
    channels: { mic: emptyChannel(), system: emptyChannel() },
    question: null,
    offline: null,
    notice: null,
    requestPending: false,
    nextSource: "window",
    nextLength: "auto",
    ended: null,
    counter: 0,
  };
}

/** Two finished lines from the same person this close together read as one turn. */
const MERGE_WINDOW_MS = 6000;
/** Speech-engine loudness above which a channel is proven to be hearing something. */
const HEARD_SOMETHING = 0.002;

const NETWORK_ERROR = /network|offline|internet|dns|resolve|connect|timed? ?out|unreachable|502|503|504/i;

function nextId(state: LiveState, prefix: string): [string, number] {
  const counter = state.counter + 1;
  return [`${prefix}${counter}`, counter];
}

function patchChannel(state: LiveState, channel: ChannelKey, patch: Partial<ChannelView>): LiveState {
  return { ...state, channels: { ...state.channels, [channel]: { ...state.channels[channel], ...patch } } };
}

function lastStreamingIndex(items: FeedItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.kind === "answer" && item.status === "streaming") return i;
  }
  return -1;
}

function replaceAt(items: FeedItem[], index: number, item: FeedItem): FeedItem[] {
  const next = items.slice();
  next[index] = item;
  return next;
}

export function reduce(state: LiveState, action: LiveAction): LiveState {
  switch (action.type) {
    case "session-started":
      return { ...state, session: action.info, startedAtMs: action.atMs, ended: null };

    case "seed-transcript": {
      if (state.items.length > 0 || action.turns.length === 0) return state;
      let counter = state.counter;
      const items: FeedItem[] = action.turns.map((turn) => {
        counter += 1;
        return { kind: "turn", id: `t${counter}`, who: turn.channel === "mic" ? "me" : "them", text: turn.text, atMs: action.atMs };
      });
      return { ...state, items, counter };
    }

    case "transcript": {
      const who = action.channel === "mic" ? "me" : "them";
      const text = action.text.trim();
      if (!action.isFinal) {
        if (text.length === 0) return state.interim?.who === who ? { ...state, interim: null } : state;
        return { ...state, interim: { who, text } };
      }
      const interim = state.interim?.who === who ? null : state.interim;
      if (text.length === 0) return { ...state, interim };

      const last = state.items[state.items.length - 1];
      const joins = last && last.kind === "turn" && last.who === who && action.atMs - last.atMs < MERGE_WINDOW_MS;
      let items: FeedItem[];
      let counter = state.counter;
      let turnId: string;
      if (joins) {
        turnId = last.id;
        items = replaceAt(state.items, state.items.length - 1, { ...last, text: `${last.text} ${text}`, atMs: action.atMs });
      } else {
        counter += 1;
        turnId = `t${counter}`;
        items = [...state.items, { kind: "turn", id: turnId, who, text, atMs: action.atMs }];
      }
      const question = who === "them" && looksLikeQuestion(text) ? { turnId, text } : state.question;
      return { ...state, items, interim, question, counter };
    }

    case "answer-requested":
      return { ...state, requestPending: true, nextSource: action.source, nextLength: action.length };

    case "answer-request-settled":
      return { ...state, requestPending: false, nextSource: "window", nextLength: "auto" };

    case "answer-started": {
      const [id, counter] = nextId(state, "a");
      let items = state.items;
      const open = lastStreamingIndex(items);
      if (open >= 0) items = replaceAt(items, open, { ...(items[open] as Answer), status: "done" });
      const answer: Answer = {
        kind: "answer",
        id,
        question: action.question,
        cleanedQuestion: null,
        text: "",
        status: "streaming",
        canned: false,
        error: null,
        source: state.nextSource,
        length: state.nextLength,
        atMs: action.atMs,
      };
      return {
        ...state,
        items: [...items, answer],
        question: null,
        offline: null,
        notice: null,
        nextSource: "window",
        nextLength: "auto",
        counter,
      };
    }

    case "answer-cleaned": {
      const index = lastStreamingIndex(state.items);
      if (index < 0) return state;
      const answer = state.items[index] as Answer;
      return { ...state, items: replaceAt(state.items, index, { ...answer, cleanedQuestion: action.cleaned || null }) };
    }

    case "answer-delta": {
      const index = lastStreamingIndex(state.items);
      if (index < 0) return state;
      const answer = state.items[index] as Answer;
      return { ...state, items: replaceAt(state.items, index, { ...answer, text: answer.text + action.text }) };
    }

    case "answer-done": {
      const index = lastStreamingIndex(state.items);
      if (index < 0) return state;
      const answer = state.items[index] as Answer;
      const text = action.canned ? answer.text : cleanFinishedAnswer(answer.text);
      return { ...state, items: replaceAt(state.items, index, { ...answer, text, status: "done", canned: action.canned }) };
    }

    case "answer-error": {
      const offline = NETWORK_ERROR.test(action.message) ? action.message : state.offline;
      const index = lastStreamingIndex(state.items);
      if (index >= 0) {
        const answer = state.items[index] as Answer;
        return { ...state, offline, items: replaceAt(state.items, index, { ...answer, status: "error", error: action.message }) };
      }
      const [id, counter] = nextId(state, "a");
      const failed: Answer = {
        kind: "answer", id, question: "", cleanedQuestion: null, text: "", status: "error", canned: false,
        error: action.message, source: state.nextSource, length: state.nextLength, atMs: action.atMs,
      };
      return { ...state, offline, items: [...state.items, failed], counter, nextSource: "window", nextLength: "auto" };
    }

    case "trigger-failed":
      return { ...state, notice: action.message, nextSource: "window", nextLength: "auto" };

    case "channel-starting":
      return patchChannel(state, action.channel, { status: "starting", message: null });

    case "channel-started":
      return patchChannel(state, action.channel, { status: "on", device: action.device, message: null });

    case "channel-stopped":
      return patchChannel(state, action.channel, { status: "off", level: 0, speaking: false, message: null });

    case "channel-failed":
      return patchChannel(state, action.channel, { status: "error", level: 0, speaking: false, message: action.message });

    case "channel-level": {
      const view = state.channels[action.channel];
      if (action.rms > HEARD_SOMETHING && view.status === "silent") {
        return patchChannel(state, action.channel, { level: action.rms, status: "on", message: null });
      }
      return patchChannel(state, action.channel, { level: action.rms });
    }

    case "channel-vad":
      return patchChannel(state, action.channel, { speaking: action.speaking });

    case "channel-notice": {
      const text = action.text;
      if (/format changed|reconnecting/i.test(text)) return state;
      if (/no audio heard|no audio\s+buffers/i.test(text)) {
        const view = state.channels[action.channel];
        if (view.status === "on" || view.status === "silent") {
          return patchChannel(state, action.channel, { status: "silent", message: text });
        }
        return state;
      }
      return { ...state, notice: text };
    }

    case "notice":
      return { ...state, notice: action.text };

    case "dismiss-notice":
      return { ...state, notice: null };

    case "dismiss-question":
      return { ...state, question: null };

    case "session-ended": {
      const turns = state.items.filter((item) => item.kind === "turn").length;
      const answers = state.items.filter((item) => item.kind === "answer" && item.status === "done").length;
      return {
        ...state,
        question: null,
        ended: { elapsedMs: Math.max(0, action.atMs - state.startedAtMs), turns, answers },
      };
    }
  }
}

/* ---------------------------- derived views ---------------------------- */

/** The single word for "what is happening right now", used by the header pill. */
export type LivePhase = "ended" | "answering" | "question" | "offline" | "silent" | "paused" | "listening";

export function derivePhase(state: LiveState): LivePhase {
  if (state.ended) return "ended";
  if (isAnswering(state)) return "answering";
  if (state.question) return "question";
  if (state.offline) return "offline";
  if (state.channels.system.status === "error") return "silent";
  const micOff = state.channels.mic.status === "off" || state.channels.mic.status === "error";
  const systemOff = state.channels.system.status === "off";
  if (micOff && systemOff) return "paused";
  return "listening";
}

export function answersOf(state: LiveState): Answer[] {
  return state.items.filter((item): item is Answer => item.kind === "answer");
}

export function isAnswering(state: LiveState): boolean {
  return state.requestPending || state.items.some((item) => item.kind === "answer" && item.status === "streaming");
}

/** The question an answer should be retried with, if any (typed follow-ups only). */
export function retryQuestion(answer: Answer): string | null {
  return answer.source === "typed" && answer.question.trim() ? answer.question : null;
}

/** Real activity only; late meter/VAD events cannot light a stopped channel. */
export function channelHasActivity(view: ChannelView): boolean {
  return (view.status === "on" || view.status === "silent") && (view.speaking || view.level > HEARD_SOMETHING);
}
