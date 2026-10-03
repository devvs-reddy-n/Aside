import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { formatClock } from "../core/format";
import { Icon } from "../kit/Icon";
import { AnswerCard } from "./Answer";
import { answerPages, pageIndex } from "./navigation";
import { useLive, useLiveUi } from "./context";
import type { Answer, FeedItem } from "./machine";

/** How close to the bottom still counts as "following along". */
const NEAR_BOTTOM_PX = 80;

function Empty({ answersOnly }: { answersOnly: boolean }) {
  const system = useLive((state) => state.channels.system.status);
  const mic = useLive((state) => state.channels.mic.status);
  const paused = useLiveUi((state) => state.paused !== null) || (mic === "off" && system === "off");
  const title = paused ? "Listening is paused" : answersOnly ? "No answers yet" : "Listening…";
  const text = paused
    ? "Turn on the microphone or system audio above."
    : answersOnly
      ? "Your current question and answer will appear here."
      : "Speech appears here. Questions are flagged.";
  return (
    <div className="lv-empty">
      <b>{title}</b>
      <p>{text}</p>
    </div>
  );
}

function TurnRow({ item, startedAtMs, flagged }: { item: Extract<FeedItem, { kind: "turn" }>; startedAtMs: number; flagged: boolean }) {
  return (
    <div className="lv-turn" data-who={item.who} data-flagged={flagged || undefined}>
      <div className="lv-turn__meta">
        <span>{item.who === "me" ? "You" : "Them"}</span>
        <time>{formatClock(item.atMs - startedAtMs)}</time>
      </div>
      <p className="lv-turn__text selectable">{item.text}</p>
    </div>
  );
}

function ConversationFeed() {
  const items = useLive((state) => state.items);
  const interim = useLive((state) => state.interim);
  const startedAtMs = useLive((state) => state.startedAtMs);
  const flaggedTurn = useLive((state) => state.question?.turnId ?? null);
  const view = useLiveUi((state) => state.view);
  const cursor = useLiveUi((state) => state.cursor);
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const seen = useRef(0);
  const lastAnswerId = useRef<string | null>(null);
  /** While set, the feed keeps this answer's first line at the top as it grows. */
  const anchor = useRef<string | null>(null);
  const [away, setAway] = useState(0);
  const [atBottom, setAtBottom] = useState(true);
  const [flash, setFlash] = useState<string | null>(null);
  /** "2 of 5" while stepping through answers with ⌘< and ⌘>. */
  const [step, setStep] = useState<{ at: number; of: number; tick: number } | null>(null);

  const answersOnly = view === "answers";
  const shown = useMemo(() => (answersOnly ? items.filter((item) => item.kind === "answer") : items), [items, answersOnly]);
  const answers = useMemo(() => items.filter((item): item is Answer => item.kind === "answer"), [items]);
  const latestId = answers.at(-1)?.id ?? null;
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

  const toBottom = (smooth: boolean) => {
    const node = scroller.current;
    if (!node) return;
    node.scrollTo({ top: node.scrollHeight, behavior: smooth && !reduced ? "smooth" : "auto" });
  };

  // Follow the conversation while the person is at the bottom. A brand-new answer opens at its
  // first line and stays there while it streams in, until the person scrolls or it finishes.
  useLayoutEffect(() => {
    const node = scroller.current;
    if (!node) return;
    const newest = shown[shown.length - 1];
    const fresh = newest?.kind === "answer" && newest.id !== lastAnswerId.current;
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

  // ⌘< and ⌘> step through answers.
  useEffect(() => {
    if (!cursor) return;
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

  const empty = shown.length === 0 && !(interim && !answersOnly);

  return (
    <div className="lv-feedwrap" role="tabpanel" id="live-feed-panel" aria-label="Chat">
      <div ref={scroller} className="lv-feed" onScroll={onScroll} onWheel={letGo} onTouchStart={letGo} onKeyDown={letGo} role="log" aria-live="polite" aria-label="Conversation and answers" tabIndex={0}>
        {empty ? <Empty answersOnly={answersOnly} /> : null}
        {shown.map((item) =>
          item.kind === "turn" ? (
            <TurnRow key={item.id} item={item} startedAtMs={startedAtMs} flagged={item.id === flaggedTurn} />
          ) : (
            <AnswerCard key={item.id} answer={item as Answer} latest={item.id === latestId} flash={flash === item.id} />
          ),
        )}
        {interim && !answersOnly ? (
          <div className="lv-turn" data-who={interim.who} data-interim="">
            <div className="lv-turn__meta"><span>{interim.who === "me" ? "You" : "Them"}</span><em>speaking</em></div>
            <p className="lv-turn__text">{interim.text}</p>
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


function FocusedAnswer() {
  const state = useLive((state) => ({ items: state.items, question: state.question }));
  const cursor = useLiveUi((state) => state.cursor);
  const pending = useLive((state) => state.requestPending);
  const screen = useLive((state) => state.nextSource === "screen");
  const pages = useMemo(() => answerPages(state), [state.items, state.question]);
  const page = pages[pageIndex(pages.length, cursor?.index)];
  const id = page?.kind === "answer" ? page.answer.id : page?.question.turnId;
  const box = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (box.current) box.current.scrollTop = 0; }, [id]);
  return <div className="lv-feedwrap" role="tabpanel" id="live-feed-panel" aria-label="Answer">
    <div ref={box} className="lv-feed" tabIndex={0} aria-live="polite" aria-label="Current question and answer">
      {pending && screen ? <p className="lv-capture-state" role="status"><Icon name="camera" size={20} />Capturing screen…</p> : null}
      {pending && screen ? null : page?.kind === "answer" ? <AnswerCard key={page.answer.id} answer={page.answer} latest={page === pages.at(-1)} focused /> : page?.kind === "question" ? <section className="lv-current-question" aria-label="Current question"><p className="lv-current-question__label">Question</p><p className="selectable">{page.question.text}</p><p className="lv-waiting">Choose Answer or a response length below.</p></section> : !pending ? <Empty answersOnly /> : null}
    </div>
  </div>;
}

export function Feed() {
  const view = useLiveUi((state) => state.view);
  return view === "answers" ? <FocusedAnswer /> : <ConversationFeed />;
}
