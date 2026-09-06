import type { ChannelId } from "./types";

/**
 * Every event name string the Rust backend actually emits, in one place,
 * each one cited to the exact Rust source that owns it. Nothing here is
 * guessed — `audio::channel::Channel::level_event` etc. have their own
 * unit test (`event_names_are_exactly_the_strings_the_frontend_listens_for`,
 * `src-tauri/src/audio/channel.rs`) pinning these literal strings, so a
 * rename on the Rust side without updating this file would show up as a
 * silently dead listener, not a build error — keep this file in sync by
 * hand if that ever happens.
 */
export function channelEventNames(channel: ChannelId) {
  return {
    level: `${channel}-level`,
    error: `${channel}-error`,
    notice: `${channel}-notice`,
    vadState: `${channel}-vad-state`,
    vadEvent: `${channel}-vad-event`,
    transcript: `${channel}-transcript`,
    transcriptError: `${channel}-transcript-error`,
  } as const;
}

/** `llm/mod.rs`'s `EVENT_ANSWER_*` / `EVENT_QUESTION_CLEANED` constants. */
export const LLM_EVENTS = {
  answerStarted: "answer-started",
  answerDelta: "answer-delta",
  answerDone: "answer-done",
  answerError: "answer-error",
  questionCleaned: "question-cleaned",
} as const;

/** `active_session.rs`'s `EVENT_SESSION_STARTED` / `EVENT_SESSION_ENDED`. */
export const SESSION_EVENTS = {
  started: "session-started",
  ended: "session-ended",
} as const;

/** `window_memory/mod.rs`'s `EVENT_WINDOW_MEMORY` constant — fires after
 *  every Command+Enter with the latest facts/summary. The event payload's
 *  own `raw_window` is deliberately blank (the window that was just
 *  answered is gone; a fresh one has already started), so a listener that
 *  wants the live "what's in the window right now" text should re-fetch
 *  `get_window_preview` rather than read it off the event payload. */
export const WINDOW_MEMORY_EVENT = "window-memory";

/** `shortcuts.rs`'s `EVENT_OVERLAY_*` constants — the four overlay-only
 *  global shortcuts (⌘⇧A chat, ⌘H collapse, ⌘[ / ⌘] history) that carry no
 *  data, they are pure "do the thing" notifications the overlay listens
 *  for. Only the overlay window ever mounts a listener for these. */
export const OVERLAY_SHORTCUT_EVENTS = {
  toggleChat: "overlay-toggle-chat",
  toggleCollapse: "overlay-toggle-collapse",
  historyPrev: "overlay-history-prev",
  historyNext: "overlay-history-next",
} as const;

/** `stealth.rs`'s `EVENT_STEALTH_CHANGED` — payload is a boolean, true when
 *  Aside is hidden from screenshots, recordings, and meeting screen-shares. */
export const STEALTH_CHANGED_EVENT = "stealth-changed";

/** `lifecycle.rs`'s `EVENT_QUIT_REQUESTED` — open the Quit confirmation
 *  dialog. Fired by the title-bar cross, ⌘Q, and the Aside menu's Quit. */
export const QUIT_REQUESTED_EVENT = "quit-requested";
