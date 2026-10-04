import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { formatClock } from "../../core/format";
import { Icon } from "../../kit/Icon";
import { useLive, useLiveUi } from "../context";
import type { Answer, FeedItem, Turn } from "../machine";
import { AnswerCard } from "./AnswerCard";

/** How close to the bottom still counts as "following along". */
const NEAR_BOTTOM_PX = 80;

function Bubble({ turn, startedAtMs, question }: { turn: Turn; startedAtMs: number; question: boolean }) {
  return (
    <div className="lv-bubble" data-who={turn.who} data-question={question || undefined}>
      <div className="lv-bubble__meta">
        <span>{turn.who === "me" ? "You" : "Them"}</span>
        <time className="tabular">{formatClock(turn.atMs - startedAtMs)}</time>
        {question ? <em><Icon name="question" size={12} strokeWidth={2} />Question</em> : null}
      </div>
      <p className="lv-bubble__text selectable">{turn.text}</p>
    </div>
  );
}

function Quiet() {
  const paused = useLiveUi((state) => state.paused !== null);
  const bothOff = useLive((state) => state.channels.mic.status === "off" && state.channels.system.status === "off");
  const resting = paused || bothOff;
  return (
    <div className="lv-empty">
      <span className="lv-empty__icon" aria-hidden="true"><Icon name={resting ? "pause" : "wave"} size={20} strokeWidth={2} /></span>
      <b>{resting ? "Listening is paused" : "Listening…"}</b>
      <p>{resting ? "Turn on You or Them above." : "What's said shows up here. Questions are marked."}</p>
    </div>
  );
}

/**
 * The running conversation. `answers` puts each answer right under what was
 * said (the Thread layout); without it this is the plain transcript. It follows
 * along while you're at the bottom, holds a new answer at its first line while
 * it streams, offers "N new" when you've scrolled up, and scrolls to the answer
 * picked with ⌘< / ⌘>.
 */
export function Conversation({ answers: withAnswers }: { answers: boolean }) {
  const items = useLive((state) => state.items);
  const interim = useLive((state) => state.interim);
  const startedAtMs = useLive((state) => state.startedAtMs);
  const flaggedTurn = useLive((state) => state.question?.turnId ?? null);
  const cursor = useLiveUi((state) => state.cursor);
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const seen = useRef(0);
  const lastAnswerId = useRef<string | null>(null);
  /** While set, the list keeps this answer's first line at the top as it grows. */
  const anchor = useRef<string | null>(null);
  const [away, setAway] = useState(0);
  const [atBottom, setAtBottom] = useState(true);
  const [flash, setFlash] = useState<string | null>(null);
  const [step, setStep] = useState<{ at: number; of: number; tick: number } | null>(null);

  const shown = useMemo<FeedItem[]>(() => (withAnswers ? items : items.filter((item) => item.kind === "turn")), [items, withAnswers]);
  const answers = useMemo(() => items.filter((item): item is Answer => item.kind === "answer"), [items]);
  const latestId = answers.at(-1)?.id ?? null;
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

  const toBottom = (smooth: boolean) => {
    const node = scroller.current;
    if (!node) return;
    node.scrollTo({ top: node.scrollHeight, behavior: smooth && !reduced ? "smooth" : "auto" });
  };

  useLayoutEffect(() => {
    const node = scroller.current;
    if (!node) return;
    const newest = shown[shown.length - 1];
    const fresh = withAnswers && newest?.kind === "answer" && newest.id !== lastAnswerId.current;
    if (fresh) {
      lastAnswerId.current = newest.id;
      anchor.current = newest.id;
    }
    if (anchor.current) {
      const answer = shown.find((item): item is Answer => item.kind === "answer" && item.id === anchor.current);
      const card = node.querySelector<HTMLElement>(`[data-answer="${anchor.current}"]`);
      if (answer && card) {
        const top = Math.max(0, card.offsetTop - 8);
        if (fresh) node.scrollTo({ top, behavior: reduced ? "auto" : "smooth" });
        else node.scrollTop = top;
        following.current = false;
        setAtBottom(false);
        seen.current = shown.length;
        setAway(0);
        if (answer.status !== "streaming") anchor.current = null;
        return;
      }
      anchor.current = null;
    }
    if (following.current) {
      toBottom(false);
      seen.current = shown.length;
      setAway(0);
    } else {
      setAway(Math.max(0, shown.length - seen.current));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, interim]);

  // ⌘< and ⌘> step through answers in place.
  useEffect(() => {
    if (!cursor || !withAnswers) return;
    const target = answers[cursor.index];
    if (!target) return;
    const card = scroller.current?.querySelector<HTMLElement>(`[data-answer="${target.id}"]`);
    if (!card) return;
    scroller.current?.scrollTo({ top: Math.max(0, card.offsetTop - 8), behavior: reduced ? "auto" : "smooth" });
    setFlash(target.id);
    setStep({ at: cursor.index + 1, of: answers.length, tick: cursor.tick });
    const id = window.setTimeout(() => setFlash(null), 1200);
    const hide = window.setTimeout(() => setStep(null), 1600);
    return () => {
      window.clearTimeout(id);
      window.clearTimeout(hide);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cursor]);

  /** Any hands-on scrolling means the person is reading; stop holding the answer's first line in place. */
  function letGo() {
    anchor.current = null;
  }

  function onScroll() {
    const node = scroller.current;
    if (!node) return;
    const near = node.scrollHeight - node.scrollTop - node.clientHeight < NEAR_BOTTOM_PX;
    following.current = near;
    setAtBottom(near);
    if (near) {
      seen.current = shown.length;
      setAway(0);
    }
  }

  const empty = shown.length === 0 && !interim;

  return (
    <div className="lv-scrollwrap" role="tabpanel" id="live-feed-panel" aria-label={withAnswers ? "Conversation and answers" : "Transcript"}>
      <div ref={scroller} className="lv-scroll lv-convo" onScroll={onScroll} onWheel={letGo} onTouchStart={letGo} onKeyDown={letGo} role="log" aria-live="polite" tabIndex={0}>
        {empty ? <Quiet /> : null}
        {shown.map((item) =>
          item.kind === "turn" ? (
            <Bubble key={item.id} turn={item} startedAtMs={startedAtMs} question={item.id === flaggedTurn} />
          ) : (
            <AnswerCard key={item.id} answer={item} latest={item.id === latestId} flash={flash === item.id} />
          ),
        )}
        {interim ? (
          <div className="lv-bubble" data-who={interim.who} data-interim="">
            <div className="lv-bubble__meta"><span>{interim.who === "me" ? "You" : "Them"}</span><em className="lv-speaking">speaking</em></div>
            <p className="lv-bubble__text">{interim.text}</p>
          </div>
        ) : null}
      </div>
      {step ? <div key={step.tick} className="lv-stepper-pill tabular" role="status">{step.at} of {step.of}</div> : null}
      {!atBottom ? (
        <button
          type="button"
          className="lv-jump"
          onClick={() => {
            following.current = true;
            setAtBottom(true);
            toBottom(true);
            seen.current = shown.length;
            setAway(0);
          }}
        >
          <Icon name="down" size={14} />{away > 0 ? `${away} new` : "Latest"}
        </button>
      ) : null}
    </div>
  );
}
