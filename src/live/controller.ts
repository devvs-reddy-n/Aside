import type { OverlayPrefs } from "../bridge/types";
import { createStore, type Store } from "../core/store.ts";
import type { LiveBackend, ShortcutName } from "./backend";
import {
  initialLiveState,
  isAnswering,
  reduce,
  retryQuestion,
  type Answer,
  type ChannelKey,
  type LiveAction,
  type LiveState,
} from "./machine.ts";
import { answerPages, pageIndex } from "./navigation.ts";

/** Everything about the live window that is about the window itself, not the call. */
export interface LiveUi {
  booted: boolean;
  prefs: OverlayPrefs;
  ending: boolean;
  screenshotBusy: boolean;
  /** Which answer ‹ › or ⌘< / ⌘> last stepped to (0 = oldest), with a tick so repeats re-scroll. */
  cursor: { index: number; tick: number } | null;
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
  answer: () => Promise<void>;
  screenshot: () => Promise<void>;
  ask: (text: string) => Promise<boolean>;
  retry: (answer: Answer) => Promise<void>;
  end: () => Promise<void>;
  changePrefs: (patch: Partial<OverlayPrefs>) => void;
  stepAnswer: (direction: -1 | 1) => void;
  /** Stops both ears, or turns back on the ones that were on. */
  togglePause: () => Promise<void>;
  /** Puts the window back at its normal size, in a place where all of it shows. */
  resetWindow: () => Promise<void>;
}

const DEFAULT_PREFS: OverlayPrefs = { zoom_pct: 100, opacity_pct: 92, font_px: 13, language: "en" };

/** How see-through the live window may get. Text always stays solid; the person chooses the rest. */
export const MIN_OPACITY = 40;
const withReadableOpacity = (prefs: OverlayPrefs): OverlayPrefs => ({ ...prefs, opacity_pct: Math.max(MIN_OPACITY, Math.min(100, prefs.opacity_pct)) });

export function createLiveController(backend: LiveBackend): LiveController {
  const live = createStore<LiveState>(initialLiveState());
  const ui = createStore<LiveUi>({
    booted: false,
    prefs: DEFAULT_PREFS,
    ending: false,
    screenshotBusy: false,
    cursor: null,
    paused: null,
  });
  let saveTimer: number | undefined;

  const dispatch = (action: LiveAction) => {
    const previousQuestion = live.get().question?.turnId;
    live.update((state) => reduce(state, action));
    if (action.type === "answer-started" || (live.get().question && live.get().question?.turnId !== previousQuestion)) ui.set({ cursor: null });
  };

  const requestAnswer = async (source: "window" | "screen" | "typed", length: Answer["length"], run: () => Promise<{ ok: boolean; error?: string }>) => {
    dispatch({ type: "answer-requested", source, length });
    const failed = (message: string) => {
      if (source === "screen") dispatch({ type: "answer-error", message, atMs: Date.now() });
      else dispatch({ type: "trigger-failed", message });
    };
    try {
      const result = await run();
      if (!result.ok) failed(result.error ?? "Aside could not start that answer.");
      return result.ok;
    } catch (error) {
      failed(error instanceof Error ? error.message : "Aside could not start that answer.");
      return false;
    } finally {
      dispatch({ type: "answer-request-settled" });
    }
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
        ui.set({ booted: true, prefs: withReadableOpacity(boot.prefs) });
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

    async answer() {
      if (isAnswering(live.get())) return;
      await requestAnswer("window", "auto", () => backend.answerWindow());
    },

    async screenshot() {
      if (isAnswering(live.get()) || ui.get().screenshotBusy) return;
      ui.set({ screenshotBusy: true, cursor: null });
      await requestAnswer("screen", "auto", () => backend.answerScreenshot());
      ui.set({ screenshotBusy: false });
    },

    async ask(text) {
      const question = text.trim();
      if (!question || isAnswering(live.get())) return false;
      const sessionId = live.get().session?.id ?? null;
      return requestAnswer("typed", "auto", async () => {
        const result = await backend.ask(question, sessionId);
        return result.ok ? { ok: true } : { ok: false, error: result.error };
      });
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
      return controller.answer();
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
      const count = answerPages(live.get()).length;
      if (count === 0) return;
      const current = pageIndex(count, ui.get().cursor?.index);
      const index = Math.max(0, Math.min(count - 1, current + direction));
      ui.set((state) => ({ cursor: { index, tick: (state.cursor?.tick ?? 0) + 1 } }));
    },
  };

  function onShortcut(name: ShortcutName) {
    controller.stepAnswer(name === "previous" ? -1 : 1);
  }

  return controller;
}
