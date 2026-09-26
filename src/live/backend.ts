import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../bridge/api";
import { onEvent, onEvents } from "../bridge/events";
import { openPrivacySettings, type PrivacyPane } from "../bridge/system";
import { LLM_EVENTS, OVERLAY_SHORTCUT_EVENTS, SESSION_EVENTS, channelEventNames } from "../bridge/eventNames";
import type {
  ActiveSessionInfo,
  AnswerDeltaEvent,
  AnswerDoneEvent,
  AnswerErrorEvent,
  AnswerStartedEvent,
  ChannelId,
  ChannelLevelEvent,
  LiveTranscriptTurn,
  OverlayPrefs,
  QuestionCleanedEvent,
  TranscriptEvent,
  VadStatus,
} from "../bridge/types";
import type { AnswerLength, Result } from "../bridge/api";
import type { ChannelKey, LiveAction } from "./machine";

export type ShortcutName = "toggleChat" | "toggleCollapse" | "previous" | "next";

export interface Boot {
  session: ActiveSessionInfo | null;
  turns: LiveTranscriptTurn[];
  /** Channels that were already listening when this window opened. */
  listening: Partial<Record<ChannelKey, { device: string | null }>>;
  prefs: OverlayPrefs;
}

/**
 * Everything the live window needs from outside itself. The real app
 * implements this on top of the Rust backend (`tauriBackend`); a browser
 * preview implements it with a scripted pretend call (`demo.ts`), which is
 * how the layout can be checked at any window size without a real meeting.
 */
export interface LiveBackend {
  boot(): Promise<Boot>;
  /** Start delivering backend events as actions. Returns the stop function. */
  listen(dispatch: (action: LiveAction) => void, onShortcut: (name: ShortcutName) => void): () => void;
  /** Start listening on one ear (capture, then transcription). Resolves to the device name. */
  startChannel(channel: ChannelKey, session: ActiveSessionInfo | null): Promise<Result<string | null>>;
  stopChannel(channel: ChannelKey): Promise<void>;
  answerWindow(length: AnswerLength | null): Promise<Result<void>>;
  answerScreenshot(): Promise<Result<void>>;
  ask(question: string, sessionId: string | null): Promise<Result<void>>;
  end(sessionId: string | null): Promise<Result<void>>;
  savePrefs(prefs: OverlayPrefs): Promise<OverlayPrefs | null>;
  /** Puts the window back at its normal size, somewhere all of it shows. */
  resetWindow(): Promise<void>;
  focusWindow(): Promise<void>;
  startResize(): void;
  openMain(): Promise<void>;
  openPrivacySettings(pane: PrivacyPane): Promise<void>;
}

const CHANNEL_KEYS: ChannelKey[] = ["mic", "system"];
const DEFAULT_SIZE: [number, number] = [400, 640];

export const tauriBackend: LiveBackend = {
  async boot() {
    const [session, turns, status, prefs] = await Promise.all([
      api.live.activeSession(),
      api.live.transcript(),
      api.audio.status(),
      api.windows.overlayPrefs(),
    ]);
    const listening: Boot["listening"] = {};
    if (status.ok) {
      for (const channel of CHANNEL_KEYS) {
        const capturing = channel === "mic" ? status.value.mic_capturing : status.value.system_capturing;
        if (!capturing) continue;
        const info = await api.audio.info(channel as ChannelId);
        listening[channel] = { device: info.ok && info.value ? info.value.device_name : null };
      }
    }
    return {
      session: session.ok ? session.value : null,
      turns: turns.ok ? turns.value : [],
      listening,
      prefs: prefs.ok ? prefs.value : { zoom_pct: 100, opacity_pct: 95, font_px: 13, language: "en" },
    };
  },

  listen(dispatch, onShortcut) {
    const stops: Array<() => void> = [];
    for (const channel of CHANNEL_KEYS) {
      const names = channelEventNames(channel);
      stops.push(
        onEvent<ChannelLevelEvent>(names.level, (p) => dispatch({ type: "channel-level", channel, rms: p.rms })),
        onEvent<VadStatus>(names.vadState, (p) => dispatch({ type: "channel-vad", channel, speaking: p.speaking })),
        onEvent<string>(names.error, (message) => dispatch({ type: "channel-failed", channel, message })),
        onEvent<string>(names.notice, (text) => dispatch({ type: "channel-notice", channel, text })),
        onEvent<string>(names.transcriptError, (text) => dispatch({ type: "notice", text })),
        onEvent<TranscriptEvent>(names.transcript, (p) =>
          dispatch({ type: "transcript", channel, text: p.text, isFinal: p.is_final, atMs: Date.now() }),
        ),
      );
    }
    stops.push(
      onEvent<AnswerStartedEvent>(LLM_EVENTS.answerStarted, (p) => dispatch({ type: "answer-started", question: p.question, atMs: Date.now() })),
      onEvent<QuestionCleanedEvent>(LLM_EVENTS.questionCleaned, (p) => dispatch({ type: "answer-cleaned", cleaned: p.cleaned_question })),
      onEvent<AnswerDeltaEvent>(LLM_EVENTS.answerDelta, (p) => dispatch({ type: "answer-delta", text: p.text })),
      onEvent<AnswerDoneEvent>(LLM_EVENTS.answerDone, (p) => dispatch({ type: "answer-done", canned: p.canned })),
      onEvent<AnswerErrorEvent>(LLM_EVENTS.answerError, (p) => dispatch({ type: "answer-error", message: p.message, atMs: Date.now() })),
      onEvent<ActiveSessionInfo>(SESSION_EVENTS.started, (info) => dispatch({ type: "session-started", info, atMs: Date.now() })),
      onEvent<null>(SESSION_EVENTS.ended, () => dispatch({ type: "session-ended", atMs: Date.now() })),
      onEvent<string>("shortcut-notice", (text) => dispatch({ type: "notice", text })),
      onEvent(OVERLAY_SHORTCUT_EVENTS.toggleChat, () => onShortcut("toggleChat")),
      onEvent(OVERLAY_SHORTCUT_EVENTS.toggleCollapse, () => onShortcut("toggleCollapse")),
      onEvent(OVERLAY_SHORTCUT_EVENTS.historyPrev, () => onShortcut("previous")),
      onEvent(OVERLAY_SHORTCUT_EVENTS.historyNext, () => onShortcut("next")),
    );
    return onEvents(stops);
  },

  async startChannel(channel, session) {
    const pinned = await api.audio.pinnedDevice(channel);
    const started = await api.audio.start(channel, pinned.ok ? pinned.value : null);
    if (!started.ok) return started;
    // Starting capture alone is not enough: the words only reach Aside once
    // transcription is running too. If that step fails, stop the capture again
    // so the button never claims to be listening when nothing can be heard.
    const stt = await api.audio.startTranscription(channel, session?.save_transcript ? session.id : null);
    if (!stt.ok) {
      await api.audio.stop(channel);
      return { ok: false, error: stt.error };
    }
    return { ok: true, value: started.value.device_name };
  },

  async stopChannel(channel) {
    await api.audio.stopTranscription(channel);
    await api.audio.stop(channel);
  },

  answerWindow: (length) => api.live.answerWindow(length),
  answerScreenshot: () => api.live.answerScreenshot(),
  ask: (question, sessionId) => api.live.ask(question, sessionId),
  end: (sessionId) => api.live.end(sessionId),

  async savePrefs(prefs) {
    const saved = await api.windows.saveOverlayPrefs(prefs);
    return saved.ok ? saved.value : null;
  },
  async resetWindow() {
    await api.windows.setOverlayMode("dock", DEFAULT_SIZE[0], DEFAULT_SIZE[1]);
  },
  async focusWindow() {
    try {
      await getCurrentWindow().setFocus();
    } catch {
      // Focus is a nicety; typing still works once the person clicks in.
    }
  },
  startResize() {
    void getCurrentWindow().startResizeDragging("SouthEast").catch(() => undefined);
  },
  async openMain() {
    await api.windows.showMain();
  },
  openPrivacySettings,
};
