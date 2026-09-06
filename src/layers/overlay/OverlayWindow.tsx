import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSession } from "../../state/session";
import { useAudioChannel } from "../../state/audioChannel";
import { useAnswerStream } from "../../state/answers";
import { useWindowMemory } from "../../state/windowMemory";
import { invoke } from "../../platform/invoke";
import { useTauriEvent } from "../../platform/events";
import { OVERLAY_SHORTCUT_EVENTS, channelEventNames } from "../../platform/eventNames";
import type { LiveTranscriptTurn, OverlayPrefs, TranscriptEvent } from "../../platform/types";
import { useQuitShortcut } from "../../window/QuitConfirm";
import { Toolbar } from "./Toolbar";
import { SubBar } from "./SubBar";
import { OverlayAnswerCard } from "./AnswerCard";
import { MoreMenu } from "./MoreMenu";
import { ChatPane } from "./ChatPane";
import { EndConfirm } from "./EndConfirm";
import { parseAnswerText } from "./answerParse";
import { isTauriRuntime } from "../../platform/runtime";

const DEFAULT_PREFS: OverlayPrefs = { zoom_pct: 100, opacity_pct: 82, font_px: 13, language: "en" };

function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const mm = hours > 0 ? String(minutes).padStart(2, "0") : String(minutes);
  const ss = String(seconds).padStart(2, "0");
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

/**
 * Screen C — Live overlay (`future refs/live-helper-ui-spec.md` §6). Its
 * own React root (`main.tsx` picks this for the `overlay` window label),
 * running in a completely separate webview from `main` — every piece of
 * session/audio/answer state here comes back from Rust the same way it
 * does on the Create window, not from any shared JS state between windows.
 */
export function OverlayWindow() {
  useQuitShortcut();
  const session = useSession();
  const mic = useAudioChannel("mic");
  const system = useAudioChannel("system");
  const answer = useAnswerStream();
  const windowMemory = useWindowMemory();

  const [collapsed, setCollapsed] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [endConfirmOpen, setEndConfirmOpen] = useState(false);
  const [manualClear, setManualClear] = useState(false);
  const [viewIndex, setViewIndex] = useState<number | null>(null);
  const [prefs, setPrefs] = useState<OverlayPrefs>(DEFAULT_PREFS);
  const [triggerError, setTriggerError] = useState<string | null>(null);
  const [screenshotBusy, setScreenshotBusy] = useState(false);
  const [interimOther, setInterimOther] = useState("");
  const [turns, setTurns] = useState<LiveTranscriptTurn[]>([]);

  const moreButtonRef = useRef<HTMLButtonElement>(null);
  const [moreMenuStyle, setMoreMenuStyle] = useState<React.CSSProperties>({});
  const connectedAtMs = useRef(Date.now()).current;
  const [now, setNow] = useState(Date.now());
  const prevStreamingRef = useRef(false);

  // The overlay's window is created transparent (`commands::overlay::open_overlay`)
  // so the dark-glass chrome below reads as floating glass, not a flat
  // opaque panel — this webview's own `<html>`/`<body>` must stay
  // transparent too, or their default paint would cover the whole window.
  // Scoped to this one webview instance only (`main`'s DOM is entirely
  // separate), so this can never affect the Create window.
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
  }, []);

  // Overlay glass opacity / zoom / font size (spec §9 rows 3-5), persisted
  // locally (not per session) — load once, then keep the CSS custom
  // properties in sync with whatever the More menu's steppers set.
  useEffect(() => {
    void invoke<OverlayPrefs>("get_overlay_prefs").then((result) => {
      if (result.ok) setPrefs(result.value);
    });
  }, []);
  useEffect(() => {
    document.documentElement.style.setProperty("--ov-opacity", String(prefs.opacity_pct / 100));
    document.documentElement.style.setProperty("--ov-zoom", String(prefs.zoom_pct / 100));
    document.documentElement.style.setProperty("--ov-font-px", `${prefs.font_px}px`);
  }, [prefs]);

  // Anchor the More panel below its trigger and inside this window. The old
  // upward calculation placed this top-toolbar menu outside the webview,
  // making it look blocked even though it had technically opened.
  useEffect(() => {
    if (!moreOpen || !moreButtonRef.current) return;
    const rect = moreButtonRef.current.getBoundingClientRect();
    setMoreMenuStyle({
      right: Math.max(8, window.innerWidth - rect.right),
      top: Math.max(8, Math.min(rect.bottom + 8, window.innerHeight - 300)),
      transformOrigin: "top right",
    });
  }, [moreOpen]);

  // Elapsed-time timer for the red Timer/End pill — ticks locally from the
  // moment this window mounted, which is effectively "when Connect was
  // pressed" since `open_overlay` is called right alongside it.
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  // Opening Chat while Collapsed used to render nothing: `bodyHasContent`
  // went true (chat is content) but `bodyExpanded` stayed false (still
  // collapsed), so `.ov-body--collapsed` clipped the whole pane to zero
  // height — the conversation was there, just invisible. Opening Chat now
  // always un-collapses; closing it leaves Collapse exactly as the person
  // left it.
  const toggleChat = useCallback(() => {
    setChatOpen((open) => {
      if (!open) setCollapsed(false);
      return !open;
    });
  }, []);

  // Global shortcuts routed through Rust (`shortcuts.rs`) — these four
  // carry no data, they are pure "do the thing" notifications.
  useTauriEvent(OVERLAY_SHORTCUT_EVENTS.toggleChat, toggleChat);
  useTauriEvent(OVERLAY_SHORTCUT_EVENTS.toggleCollapse, () => setCollapsed((c) => !c));
  useTauriEvent(OVERLAY_SHORTCUT_EVENTS.historyPrev, () => goPrev());
  useTauriEvent(OVERLAY_SHORTCUT_EVENTS.historyNext, () => goNext());

  // Spec §6.3: "Interim STT text may sit as a muted one-line 'Hearing…'
  // under the toolbar; it must never replace a committed Question." Only
  // the OTHER PARTY (system audio) channel counts — "The user's own mic
  // transcript is not the question."
  useTauriEvent<TranscriptEvent>(channelEventNames("system").transcript, (payload) => {
    setInterimOther(payload.is_final ? "" : payload.text);
    if (payload.is_final) setTurns((current) => [...current, { channel: "system" as const, text: payload.text }].slice(-160));
  });
  useTauriEvent<TranscriptEvent>(channelEventNames("mic").transcript, (payload) => {
    if (payload.is_final) setTurns((current) => [...current, { channel: "mic" as const, text: payload.text }].slice(-160));
  });
  useEffect(() => { void (async () => {
    const result = await invoke<LiveTranscriptTurn[]>("get_live_transcript");
    if (result.ok) setTurns(result.value);
  })(); }, []);

  const goPrev = useCallback(() => {
    setManualClear(false);
    setViewIndex((idx) => {
      if (answer.history.length === 0) return idx;
      if (idx === null) return answer.history.length - 1;
      return Math.max(0, idx - 1);
    });
  }, [answer.history.length]);

  const goNext = useCallback(() => {
    setManualClear(false);
    setViewIndex((idx) => {
      if (idx === null) return null;
      if (idx >= answer.history.length - 1) return null;
      return idx + 1;
    });
  }, [answer.history.length]);

  // A fresh answer starting always brings the view back to "live," even if
  // the person was browsing an older one with Prev/Next — and, since the
  // answer would otherwise be silently clipped to nothing (same bug class
  // as Chat above), also un-collapses so pressing Answer while Collapsed
  // actually shows the answer instead of generating one nobody can see.
  useEffect(() => {
    if (answer.streaming && !prevStreamingRef.current) {
      setViewIndex(null);
      setManualClear(false);
      setCollapsed(false);
    }
    prevStreamingRef.current = answer.streaming;
  }, [answer.streaming]);

  const viewed =
    viewIndex !== null && answer.history[viewIndex]
      ? {
          question: answer.history[viewIndex].question,
          cleanedQuestion: answer.history[viewIndex].cleanedQuestion,
          text: answer.history[viewIndex].text,
          streaming: false,
          canned: answer.history[viewIndex].canned,
          error: null as string | null,
        }
      : {
          question: answer.question,
          cleanedQuestion: answer.cleanedQuestion,
          text: answer.text,
          streaming: answer.streaming,
          canned: answer.canned,
          error: answer.error,
        };

  const hasContent = !manualClear && (viewed.question != null || viewed.text.trim().length > 0 || viewed.streaming);
  // Two different questions, kept separate on purpose (states to keep
  // working, per the restyle brief): "is there anything to show at all"
  // (the true empty state — toolbar only, no body, no placeholder blob) vs.
  // "is the person currently hiding it with Collapse." The body only stays
  // mounted while there is content, so its clip-path/opacity transition
  // (DESIGN.md §8) has something to animate and stays interruptible when
  // Collapse is toggled rapidly — it is never unmounted just because
  // Collapse is on.
  const bodyHasContent = chatOpen || hasContent;
  const bodyExpanded = !collapsed && bodyHasContent;
  const listening = mic.speaking || system.speaking;

  // Spec's "Current conversation" readout — visible accumulation before
  // ⌘↩, in plain words, never the technical "raw window buffer" name. Only
  // worth polling while there's nothing already on screen to show instead.
  useEffect(() => {
    if (bodyHasContent) return;
    const id = window.setInterval(() => void windowMemory.refresh(), 5000);
    return () => window.clearInterval(id);
  }, [bodyHasContent, windowMemory]);
  const canPrev = answer.history.length > 0 && (viewIndex === null || viewIndex > 0);
  const canNext = viewIndex !== null;

  // Real window resize (spec §1: "Min ~720×48 (toolbar only); expands down
  // to ~720×520") — the frontend decides *whether* to expand, this just
  // makes the real OS window match.
  useEffect(() => {
    void invoke("resize_overlay", { width: 720, height: bodyExpanded ? 480 : 64 });
  }, [bodyExpanded]);

  const triggerAnswer = useCallback(async (length: "small" | "medium" | "long" | null = null) => {
    setTriggerError(null);
    const result = await invoke<void>("trigger_window_answer", { length });
    if (!result.ok) setTriggerError(result.error);
  }, []);

  const triggerScreenshot = useCallback(async () => {
    setTriggerError(null);
    setScreenshotBusy(true);
    const result = await invoke<void>("trigger_screenshot_answer");
    setScreenshotBusy(false);
    if (!result.ok) setTriggerError(result.error);
  }, []);

  // The two controls at the left of the toolbar are deliberately independent:
  // a person can pause their microphone without losing the other side of the
  // call, or restart system audio after granting macOS permission. Starting
  // capture alone is not enough — the transcription route must start too.
  // If that second step fails, stop the new capture again so the button never
  // claims it is listening when no words can reach Aside.
  const toggleAudioChannel = useCallback(
    async (channel: typeof mic) => {
      setTriggerError(null);
      if (channel.capturing) {
        const stt = await invoke<void>("stop_channel_transcription", { channel: channel.channel });
        await channel.stop();
        if (!stt.ok) setTriggerError(stt.error);
        return;
      }

      const started = await channel.start();
      if (!started) {
        setTriggerError(channel.error ?? `Could not start ${channel.channel === "mic" ? "the microphone" : "system audio"}.`);
        return;
      }

      const stt = await invoke<void>("start_channel_transcription", {
        channel: channel.channel,
        sessionId: session.active?.save_transcript ? session.active.id : null,
      });
      if (!stt.ok) {
        await channel.stop();
        setTriggerError(stt.error);
      }
    },
    [mic, session.active],
  );

  const startResize = useCallback(() => {
    if (isTauriRuntime()) void getCurrentWindow().startResizeDragging("SouthEast");
  }, []);

  const sendChatMessage = useCallback(
    async (message: string) => {
      await answer.ask(message, session.active?.id ?? null);
    },
    [answer, session.active],
  );

  const changePrefs = useCallback(async (next: OverlayPrefs) => {
    setPrefs(next);
    const result = await invoke<OverlayPrefs>("set_overlay_prefs", { prefs: next });
    if (result.ok) setPrefs(result.value);
  }, []);

  const copyVisible = useCallback(() => {
    const parts: string[] = [];
    const shownQuestion = viewed.cleanedQuestion ?? viewed.question;
    if (shownQuestion) parts.push(`Question: ${shownQuestion}`);
    const parsed = parseAnswerText(viewed.text);
    if (parsed.prose) parts.push(`Answer: ${parsed.prose}`);
    if (parsed.keySteps) parts.push(`Key Steps:\n${parsed.keySteps.map((s) => `- ${s}`).join("\n")}`);
    if (parsed.code) parts.push(`Code (${parsed.code.language ?? "plain"}):\n${parsed.code.code}`);
    void navigator.clipboard.writeText(parts.join("\n\n"));
  }, [viewed]);

  const confirmEnd = useCallback(async () => {
    setEndConfirmOpen(false);
    await invoke<void>("end_live_session", { sessionId: session.active?.id ?? null });
  }, [session.active]);

  return (
    <div className="ov-app">
      <Toolbar
        listening={listening}
        mic={mic}
        system={system}
        onToggleMic={() => void toggleAudioChannel(mic)}
        onToggleSystem={() => void toggleAudioChannel(system)}
        onAnswer={() => void triggerAnswer()}
        onAnswerLength={(length) => void triggerAnswer(length)}
        answering={answer.streaming}
        onScreenshot={() => void triggerScreenshot()}
        screenshotBusy={screenshotBusy}
        chatOpen={chatOpen}
        onToggleChat={toggleChat}
        opacityPct={prefs.opacity_pct}
        onOpacityChange={(opacity_pct) => void changePrefs({ ...prefs, opacity_pct })}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        moreOpen={moreOpen}
        onToggleMore={() => setMoreOpen((open) => !open)}
        moreButtonRef={moreButtonRef}
        elapsedLabel={formatElapsed(now - connectedAtMs)}
        onEndClick={() => setEndConfirmOpen(true)}
      />

      {!chatOpen && !collapsed && interimOther.trim().length > 0 && viewed.question == null ? (
        <p className="ov-hearing">Hearing… {interimOther}</p>
      ) : !chatOpen && !collapsed && !bodyHasContent && windowMemory.rawWindow.trim().length > 0 ? (
        <p className="ov-hearing" title={windowMemory.rawWindow}>
          Current conversation · {windowMemory.rawWindow.trim().split(/\s+/).length} words · Listening…
        </p>
      ) : null}

      {triggerError ? <p className="ov-error-line">{triggerError}</p> : null}

      {bodyHasContent ? (
        <div className={`ov-body${bodyExpanded ? "" : " ov-body--collapsed"}`}>
          <SubBar
            canPrev={canPrev}
            canNext={canNext}
            onPrev={goPrev}
            onNext={goNext}
            onClear={() => setManualClear(true)}
            onCopy={copyVisible}
            generating={answer.streaming && viewIndex === null}
          />
          <div className="ov-card">
            {chatOpen ? (
              <ChatPane
                disabled={answer.streaming}
                streaming={answer.streaming}
                hasAnyMessage={answer.history.length > 0 || answer.question != null}
                turns={turns}
                onSend={(message) => void sendChatMessage(message)}
              />
            ) : (
              <OverlayAnswerCard
                question={viewed.question}
                cleanedQuestion={viewed.cleanedQuestion}
                text={viewed.text}
                streaming={viewed.streaming}
                canned={viewed.canned}
                error={viewed.error}
              />
            )}
          </div>
        </div>
      ) : null}

      {moreOpen ? (
        <MoreMenu
          style={moreMenuStyle}
          prefs={prefs}
          onChangePrefs={(next) => void changePrefs(next)}
          onDashboard={() => void invoke("show_main_window")}
          onEdit={() => void invoke("show_main_window")}
          accountLabel="This Mac (local only)"
        />
      ) : null}

      {endConfirmOpen ? <EndConfirm onConfirm={() => void confirmEnd()} onCancel={() => setEndConfirmOpen(false)} /> : null}

      <button
        type="button"
        className="ov-resize-grip"
        title="Resize window"
        aria-label="Resize window from the lower-right corner"
        onPointerDown={startResize}
      >
        <span aria-hidden="true" />
      </button>
    </div>
  );
}
