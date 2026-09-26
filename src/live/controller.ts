import type { OverlayPrefs } from "../bridge/types";
import { createStore, type Store } from "../core/store";
import { takeHandedOffLength } from "../core/answerLength";
import { loadLaunchPrefs, saveLaunchPrefs } from "../core/launchPrefs";
import type { LiveBackend, ShortcutName } from "./backend";
import {
  answersOf,
  initialLiveState,
  isAnswering,
  reduce,
  retryQuestion,
  type Answer,
  type ChannelKey,
  type LiveAction,
  type LiveState,
} from "./machine";
import type { AnswerLength } from "../bridge/api";

/** Everything about the live window that is about the window itself, not the call. */
export interface LiveUi {
  booted: boolean;
  prefs: OverlayPrefs;
  ending: boolean;
  screenshotBusy: boolean;
  /** Bumped to ask the footer to focus its follow-up box. */
  askTick: number;
  /** Which answer ⌘[ / ⌘] last stepped to (0 = oldest), with a tick so repeats re-scroll. */
  cursor: { index: number; tick: number } | null;
  /** "all" shows what was said and the answers; "answers" hides the running transcript. */
  view: "all" | "answers";
  /** The length the Answer button asks for. */
  length: "auto" | "small" | "medium" | "long";
  /** Ears that were on when listening was paused, so Resume can turn the same ones back on. */
  paused: ChannelKey[] | null;
}

export interface LiveController {
  live: Store<LiveState>;
  ui: Store<LiveUi>;
  backend: LiveBackend;
  dispatch: (action: LiveAction) => void;
  /** Boot and start listening. Returns the stop function. */
  start: () => () => void;
  toggleChannel: (channel: ChannelKey) => Promise<void>;
  answer: (length?: AnswerLength) => Promise<void>;
  screenshot: () => Promise<void>;
  ask: (text: string) => Promise<boolean>;
  retry: (answer: Answer) => Promise<void>;
  end: () => Promise<void>;
  changePrefs: (patch: Partial<OverlayPrefs>) => void;
  focusAsk: () => void;
  stepAnswer: (direction: -1 | 1) => void;
  setView: (view: "all" | "answers") => void;
  setLength: (length: LiveUi["length"]) => void;
  /** Stops both ears, or turns back on the ones that were on. */
  togglePause: () => Promise<void>;
  /** Puts the window back at its normal size, in a place where all of it shows. */
  resetWindow: () => Promise<void>;
}

const DEFAULT_PREFS: OverlayPrefs = { zoom_pct: 100, opacity_pct: 97, font_px: 13, language: "en" };

/** The live window is never less than 94% solid, so answers stay readable over a busy screen. Older saved values below that are lifted here. */
export const MIN_OPACITY = 94;
const withReadableOpacity = (prefs: OverlayPrefs): OverlayPrefs => ({ ...prefs, opacity_pct: Math.max(MIN_OPACITY, Math.min(100, prefs.opacity_pct)) });

export function createLiveController(backend: LiveBackend): LiveController {
  const live = createStore<LiveState>(initialLiveState());
  const ui = createStore<LiveUi>({
    booted: false,
    prefs: DEFAULT_PREFS,
    ending: false,
    screenshotBusy: false,
    askTick: 0,
    cursor: null,
    view: "all",
    length: "auto",
    paused: null,
  });
  let saveTimer: number | undefined;

  const dispatch = (action: LiveAction) => live.update((state) => reduce(state, action));

  const requestAnswer = async (source: "window" | "screen" | "typed", length: AnswerLength | "auto", run: () => Promise<{ ok: boolean; error?: string }>) => {
    dispatch({ type: "answer-requested", source, length });
    const result = await run();
    if (!result.ok) dispatch({ type: "trigger-failed", message: result.error ?? "Aside could not start that answer." });
  };

  const controller: LiveController = {
    live,
    ui,
    backend,
    dispatch,

    start() {
      const stopListening = backend.listen(dispatch, (name) => onShortcut(name));
      void (async () => {
        const boot = await backend.boot();
        const now = Date.now();
        dispatch({ type: "session-started", info: boot.session, atMs: now });
        dispatch({ type: "seed-transcript", turns: boot.turns, atMs: now });
        for (const channel of ["mic", "system"] as ChannelKey[]) {
          const on = boot.listening[channel];
          if (on) dispatch({ type: "channel-started", channel, device: on.device });
        }
        const launch = loadLaunchPrefs();
        // The copilot's default answer length, handed over by the Start panel (once).
        const length = takeHandedOffLength() ?? "auto";
        ui.set({ booted: true, prefs: withReadableOpacity(boot.prefs), view: launch.answersOnly ? "answers" : "all", length });
      })();
      return () => {
        stopListening();
        window.clearTimeout(saveTimer);
      };
    },

    async toggleChannel(channel) {
      const view = live.get().channels[channel];
      if (view.status === "starting") return;
      if (view.status === "on" || view.status === "silent") {
        await backend.stopChannel(channel);
        dispatch({ type: "channel-stopped", channel });
        return;
      }
      if (ui.get().paused) ui.set({ paused: null });
      dispatch({ type: "channel-starting", channel });
      const result = await backend.startChannel(channel, live.get().session);
      if (result.ok) dispatch({ type: "channel-started", channel, device: result.value });
      else dispatch({ type: "channel-failed", channel, message: result.error });
    },

    async answer(length) {
      if (isAnswering(live.get())) return;
      const remembered = ui.get().length;
      const chosen: AnswerLength | null = length ?? (remembered === "auto" ? null : remembered);
      await requestAnswer("window", chosen ?? "auto", () => backend.answerWindow(chosen));
    },

    async screenshot() {
      if (isAnswering(live.get()) || ui.get().screenshotBusy) return;
      ui.set({ screenshotBusy: true });
      await requestAnswer("screen", "auto", () => backend.answerScreenshot());
      ui.set({ screenshotBusy: false });
    },

    async ask(text) {
      const question = text.trim();
      if (!question || isAnswering(live.get())) return false;
      const sessionId = live.get().session?.id ?? null;
      let ok = true;
      await requestAnswer("typed", "auto", async () => {
        const result = await backend.ask(question, sessionId);
        ok = result.ok;
        return result.ok ? { ok: true } : { ok: false, error: result.error };
      });
      return ok;
    },

    async retry(answer) {
      if (isAnswering(live.get())) return;
      if (answer.source === "screen") return controller.screenshot();
      const question = retryQuestion(answer) ?? (answer.question.trim() || null);
      if (question) {
        const sessionId = live.get().session?.id ?? null;
        await requestAnswer(answer.source, answer.length, () => backend.ask(question, sessionId));
        return;
      }
      return controller.answer(answer.length === "auto" ? undefined : answer.length);
    },

    async end() {
      if (ui.get().ending) return;
      ui.set({ ending: true });
      const result = await backend.end(live.get().session?.id ?? null);
      if (!result.ok) {
        ui.set({ ending: false });
        dispatch({ type: "notice", text: `Could not end the session: ${result.error}` });
      }
    },

    changePrefs(patch) {
      const next = withReadableOpacity({ ...ui.get().prefs, ...patch });
      ui.set({ prefs: next });
      window.clearTimeout(saveTimer);
      saveTimer = window.setTimeout(() => {
        void backend.savePrefs(next).then((saved) => {
          if (saved) ui.set({ prefs: withReadableOpacity(saved) });
        });
      }, 300);
    },

    focusAsk() {
      void backend.focusWindow();
      ui.set((state) => ({ askTick: state.askTick + 1 }));
    },

    setView(view) {
      ui.set({ view });
      saveLaunchPrefs({ answersOnly: view === "answers" });
    },

    setLength(length) {
      ui.set({ length });
    },

    async togglePause() {
      const resume = ui.get().paused;
      if (resume) {
        ui.set({ paused: null });
        for (const channel of resume) await controller.toggleChannel(channel);
        return;
      }
      const running = (["mic", "system"] as ChannelKey[]).filter((channel) => {
        const status = live.get().channels[channel].status;
        return status === "on" || status === "silent";
      });
      if (running.length === 0) return;
      ui.set({ paused: running });
      for (const channel of running) await controller.toggleChannel(channel);
    },

    async resetWindow() {
      await backend.resetWindow();
    },

    stepAnswer(direction) {
      const count = answersOf(live.get()).length;
      if (count === 0) return;
      const current = ui.get().cursor?.index ?? count;
      const index = Math.max(0, Math.min(count - 1, current + direction));
      ui.set((state) => ({ cursor: { index, tick: (state.cursor?.tick ?? 0) + 1 } }));
    },
  };

  function onShortcut(name: ShortcutName) {
    if (name === "toggleChat") controller.focusAsk();
    else if (name === "previous" || name === "next") controller.stepAnswer(name === "previous" ? -1 : 1);
  }

  return controller;
}
