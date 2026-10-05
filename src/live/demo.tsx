/**
 * A pretend call for checking the live window in a plain browser tab
 * (`npm run dev`, then open `/?demo=live`). Development only: it is never
 * reachable from the real app. Add `&scn=silent`, `&scn=offline` or
 * `&scn=mic` to see the recovery cards, `&alpha=40..100` for the Background
 * setting and `&font=12..20` for Text size.
 */
import { useMemo } from "react";
import type { OverlayPrefs } from "../bridge/types";
import type { LiveBackend } from "./backend";
import { createLiveController } from "./controller";
import { LiveApp } from "./LiveApp";
import type { ChannelKey, LiveAction } from "./machine";

const SCRIPT: Array<{ at: number; channel: ChannelKey; text: string; final: boolean }> = [
  { at: 1200, channel: "system", text: "Thanks for making time today. I'd like to start with a quick", final: false },
  { at: 2600, channel: "system", text: "Thanks for making time today. I'd like to start with a quick intro from your side.", final: true },
  { at: 4200, channel: "mic", text: "Sure, happy to. I'm a platform engineer with about eight years of experience.", final: true },
  { at: 6500, channel: "system", text: "Great. So how would you design a rate limiter for a public", final: false },
  { at: 8200, channel: "system", text: "Great. So how would you design a rate limiter for a public API that has to work across many servers?", final: true },
];

const ANSWER = [
  "I'd start with a token bucket per API key, because it allows short bursts while holding a steady average rate. ",
  "To share it across servers I'd keep the counters in Redis and update them with one atomic script, so two servers never race on the same key.\n\n",
  "Key steps:\n- Identify each caller by API key\n- Refill tokens on a fixed schedule\n- Reject with a 429 and a Retry-After header when the bucket is empty\n\n",
  "```python\ndef allow(key, rate, burst, now):\n    tokens, last = redis.get(key)  # atomic in Lua\n    tokens = min(burst, tokens + (now - last) * rate)\n    if tokens < 1:\n        return False\n    redis.set(key, tokens - 1, now)\n    return True\n```",
];

function makeBackend(scn: string | null, alpha: number, font: number): LiveBackend {
  let dispatch: (action: LiveAction) => void = () => undefined;
  const on: Record<ChannelKey, boolean> = { mic: scn !== "mic", system: true };
  let prefs: OverlayPrefs = { zoom_pct: 100, opacity_pct: alpha, font_px: font, language: "en" };
  let streamTimer: number | undefined;

  const stream = (question: string) => {
    if (scn === "offline") {
      dispatch({ type: "answer-started", question, atMs: Date.now() });
      window.setTimeout(() => dispatch({ type: "answer-error", message: "error sending request: dns error: failed to lookup address", atMs: Date.now() }), 700);
      return;
    }
    dispatch({ type: "answer-started", question, atMs: Date.now() });
    let index = 0;
    const chars = ANSWER.join("").split("");
    streamTimer = window.setInterval(() => {
      const piece = chars.slice(index, index + 5).join("");
      index += 5;
      if (piece) dispatch({ type: "answer-delta", text: piece });
      if (index >= chars.length) {
        window.clearInterval(streamTimer);
        dispatch({ type: "answer-done", canned: false });
      }
    }, 40);
  };

  return {
    async boot() {
      return {
        session: { id: "demo", session_type: "interview", company: "Nimbus", job_description: null, instructions: null, save_transcript: true },
        turns: [],
        listening: {
          ...(on.mic ? { mic: { device: "MacBook Pro Microphone" } } : {}),
          ...(on.system ? { system: { device: "MacBook Pro Speakers" } } : {}),
        },
        prefs,
      };
    },
    listen(d) {
      dispatch = d;
      const timers = SCRIPT.map((step) =>
        window.setTimeout(() => d({ type: "transcript", channel: step.channel, text: step.text, isFinal: step.final, atMs: Date.now() }), step.at),
      );
      if (scn === "silent") {
        timers.push(window.setTimeout(() => d({ type: "channel-notice", channel: "system", text: "No audio heard from this device yet." }), 3000));
      }
      if (scn === "mic") {
        timers.push(window.setTimeout(() => d({ type: "channel-failed", channel: "mic", message: "Microphone permission denied" }), 1500));
      }
      const level = window.setInterval(() => {
        (["mic", "system"] as ChannelKey[]).forEach((channel) => {
          if (on[channel]) d({ type: "channel-level", channel, rms: scn === "silent" && channel === "system" ? 0 : Math.random() * 0.06 });
        });
      }, 120);
      return () => {
        timers.forEach((t) => window.clearTimeout(t));
        window.clearInterval(level);
        window.clearInterval(streamTimer);
      };
    },
    async startChannel(channel) {
      on[channel] = true;
      await new Promise((r) => setTimeout(r, 400));
      return { ok: true, value: channel === "mic" ? "MacBook Pro Microphone" : "MacBook Pro Speakers" };
    },
    async stopChannel(channel) {
      on[channel] = false;
    },
    async answerWindow() {
      stream("How would you design a rate limiter for a public API?");
      return { ok: true, value: undefined };
    },
    async answerScreenshot() {
      await new Promise((r) => setTimeout(r, 500));
      stream("screen");
      return { ok: true, value: undefined };
    },
    async ask(question) {
      stream(question);
      return { ok: true, value: undefined };
    },
    async end() {
      dispatch({ type: "session-ended", atMs: Date.now() });
      return { ok: true, value: undefined };
    },
    async savePrefs(next) {
      prefs = next;
      return next;
    },
    async resetWindow() {},
    async focusWindow() {},
    startResize() {},
    async openMain() {},
    async openPrivacySettings() {},
  };
}

export default function DemoLive() {
  const controller = useMemo(() => {
    const params = new URLSearchParams(window.location.search);
    const alpha = Math.max(40, Math.min(100, Number(params.get("alpha")) || 92));
    const font = Math.max(12, Math.min(20, Number(params.get("font")) || 13));
    return createLiveController(makeBackend(params.get("scn"), alpha, font));
  }, []);
  return <LiveApp controller={controller} />;
}
